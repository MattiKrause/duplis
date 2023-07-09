mod set_order;
mod set_consumer;
mod parse_cli;
mod os;


use std::collections::HashMap;
use std::ffi::{OsString};
use std::hash::Hasher;


use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime};


use crate::parse_cli::ExecutionPlan;

pub enum Recoverable<R, F> {
    Recoverable(R), Fatal(F)
}

#[derive(Clone, Debug)]
pub struct LinkedPath(Option<Arc<LinkedPath>>, OsString);

enum HashFileError {
    IO(std::io::Error),
    FileChanged,
}

impl From<std::io::Error> for HashFileError {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

impl LinkedPath {
    fn push_full_to_buf(&self, buf: &mut PathBuf) {
        if let Some(ancestor) = &self.0 {
            ancestor.push_full_to_buf(buf);
        }
        buf.push(&self.1);
    }

    fn to_push_buf(&self) -> PathBuf {
        let mut path_buf = PathBuf::new();
        self.push_full_to_buf(&mut path_buf);
        path_buf
    }

    fn from_path_buf(buf: &PathBuf) -> Arc<Self>  {
        buf.iter().map(ToOwned::to_owned)
            .fold(None, |acc, res| Some(Arc::new(LinkedPath(acc, res))))
            .expect("empty path")
    }

    fn root(dir: &str) -> Arc<Self> {
        Arc::new(Self(None, OsString::from(dir)))
    }
}

#[derive(Debug, Clone)]
pub struct HashedFile {
    file_version_timestamp: Option<SystemTime>,
    file_path: LinkedPath,
}

pub type BoxErr = Box<dyn std::error::Error>;

fn main() {
    // shared-imm-state: Regex etc.
    // work queue: 1 Thread -> one working thread
    // single ui responsive

    env_logger::builder().init();
    // find file -> hash file -> lookup hash in hashmap -> equals check? -> confirm? -> needs accumulate(i.e. for sort)? -> execute action
    let ExecutionPlan { dirs, recursive_dirs, follow_symlinks: _, mut order_set, action: mut file_set_action } = parse_cli::parse().unwrap();

    let (files_send, files_rev) = flume::unbounded();

    for (dir, rec) in dirs.into_iter().map(|d| (d, false)).chain(recursive_dirs.into_iter().map(|d| (d, true))) {
        produce_list(dir, rec, |file| {
            files_send.send(file).expect("sink leads to nowhere; this should not happen")
        });
    }

    drop(files_send);


    let mut path_buf = PathBuf::new();
    let mut target = HashMap::new();

    for file_path in files_rev.into_iter() {
        path_buf.clear();
        file_path.push_full_to_buf(&mut path_buf);

        match hash_file(&path_buf) {
            Ok(hash) => {
                target.entry(hash.0)
                    .or_insert(Vec::new())
                    .push(HashedFile { file_version_timestamp: hash.1, file_path });
            }
            Err(HashFileError::FileChanged) => log::warn!("file {} changed during hashing; ignoring file", path_buf.display()),
            Err(HashFileError::IO(err)) => match err.kind() {
                std::io::ErrorKind::PermissionDenied => log::warn!("no permission to access {}; ignoring file", path_buf.display()),
                std::io::ErrorKind::NotFound => {}
                _ => log::warn!("failed to access file {} due to {err}; ignoring file", path_buf.display())
            }
        }
    }

    for mut set in target.into_iter(){
        if set.1.len() <= 1 {
            continue;
        }
        for order in &mut order_set {
            order.order(&mut set.1).unwrap();
        }

        if let Err(_) = file_set_action.consume_set(set.1) {
            break
        };
    }
}

fn produce_list(path: Arc<LinkedPath>, recursive: bool, mut write_target: impl FnMut(LinkedPath)) {
    let mut dir_list = vec![path];

    let mut path_acc = PathBuf::new();
    while let Some(dir) = dir_list.pop() {
        path_acc.clear();
        dir.push_full_to_buf(&mut path_acc);
        let Ok(current_dir) = std::fs::read_dir(&path_acc) else { continue; };
        for entry in current_dir {
            let Ok(entry) = entry else { break; };
            let Ok(file_type) = entry.file_type() else { continue; };
            if file_type.is_file() {
                write_target(LinkedPath(Some(dir.clone()), entry.file_name()));
            } else if file_type.is_dir() && recursive {
                dir_list.push(Arc::new(LinkedPath(Some(dir.clone()), entry.file_name())));
            }
        }
    }
}

fn hash_file(path: impl AsRef<Path>) -> Result<(u128, Option<SystemTime>), HashFileError> {
    let mut file = std::fs::OpenOptions::new().read(true).write(false).open(path.as_ref())?;
    let metadata = file.metadata()?;
    let before_mod_time = metadata.modified().ok();// might be unavailable on the platform
    let hash_value = hash_source(&mut file)?;
    let metadata = file.metadata()?;
    let after_mod_time = metadata.modified().ok();

    if before_mod_time == after_mod_time {
        Ok((hash_value, before_mod_time))
    } else {
        Err(HashFileError::FileChanged)
    }
}

fn hash_source(mut file: impl std::io::Read) -> std::io::Result<u128> {
    let mut buf = Box::new([0; 256]);
    let mut hash = xxhash_rust::xxh3::Xxh3::default();
    while let Some(bytes_read) = Some(file.read(buf.as_mut_slice())?).filter(|amount| *amount != 0) {
        hash.write(&buf[..bytes_read]);
    }
    Ok(hash.digest128())
}