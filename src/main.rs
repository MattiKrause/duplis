#![warn(clippy::pedantic)]

mod set_order;
mod set_consumer;
mod parse_cli;
mod os;
mod file_set_refiner;
mod util;
mod file_filters;
mod error_handling;
mod file_action;


use std::collections::HashMap;
use std::ffi::{OsString};
use std::hash::{Hasher};


use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use log::LevelFilter;
use simplelog::{Config};
use crate::error_handling::AlreadyReportedError;
use crate::file_filters::FileFilter;
use crate::file_set_refiner::{FileSetRefiners};


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
    fn new_child(parent: &Arc<LinkedPath>, segment: OsString) -> Self {
        Self(Some(parent.clone()), segment)
    }

    fn write_full_to_buf(&self, buf: &mut PathBuf) {
        buf.clear();
        self._push_full_to_buf(buf);
    }

    fn _push_full_to_buf(&self, buf: &mut PathBuf) {
        if let Some(ancestor) = &self.0 {
            ancestor._push_full_to_buf(buf);
        }
        buf.push(&self.1);
    }

    fn to_push_buf(&self) -> PathBuf {
        let mut path_buf = PathBuf::new();
        self._push_full_to_buf(&mut path_buf);
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

    simplelog::SimpleLogger::init(LevelFilter::Trace, Config::default()).unwrap();
    // find file -> hash file -> lookup hash in hashmap -> equals check? -> confirm? -> needs accumulate(i.e. for sort)? -> execute action
    let ExecutionPlan { dirs, recursive_dirs, follow_symlinks, file_equals, mut order_set, action: mut file_set_action, mut file_filter } = parse_cli::parse().unwrap();

    let (files_send, files_rev) = flume::unbounded();

    for (dir, rec) in dirs.into_iter().map(|d| (d, false)).chain(recursive_dirs.into_iter().map(|d| (d, true))) {
        produce_list(dir, &mut file_filter, rec, follow_symlinks, |file| {
            files_send.send(file).expect("sink leads to nowhere; this should not happen")
        });
    }

    drop(files_send);


    let mut path_buf = PathBuf::new();
    let mut path_buf_tmp = PathBuf::new();
    let mut set_refiners = FileSetRefiners::new(file_equals.into_boxed_slice());

    let mut target: HashMap<u128, Vec<(u128, Vec<HashedFile>)>> = HashMap::new();


    for file_path in files_rev.into_iter() {
        file_path.write_full_to_buf(&mut path_buf);
        let result = place_into_file_set(file_path,&path_buf, &mut path_buf_tmp, &mut set_refiners, |hash| target.entry(hash).or_insert(Vec::new()));
        if let Err(_) = result {
            continue;
        }
    }

    for mut set in target.into_values().flat_map(|sets| sets.into_iter()){
        if set.1.len() <= 1 {
            continue;
        }
        for order in &mut order_set {
            if let Err(AlreadyReportedError {}) = order.order(&mut set.1) {
                break;
            }
        }
        if set.1.len() <= 1 {
            continue;
        }

        if let Err(_) = file_set_action.consume_set(set.1) {
            break
        };
    }
}

fn produce_list(path: Arc<LinkedPath>, file_filter: &mut FileFilter, recursive: bool, follow_symlink: bool, mut write_target: impl FnMut(LinkedPath)) {
    let mut dir_list = vec![path];

    let mut path_acc = PathBuf::new();
    while let Some(dir) = dir_list.pop() {
        dir.write_full_to_buf(&mut path_acc);
        let Ok(current_dir) = std::fs::read_dir(&path_acc) else { continue; };
        for entry in current_dir {
            let Ok(entry) = entry else { break; };
            let Ok(file_type) = entry.file_type() else { continue; };
            if file_type.is_file() {
                let file_name = entry.file_name();
                let pop_token= push_to_path(&mut path_acc, &file_name);
                let file_name = LinkedPath::new_child(&dir, file_name);
                let keep_file = file_filter.keep_file_dir_entry(&file_name, pop_token.0, entry);
                if keep_file {
                    write_target(file_name);
                }
            } else if file_type.is_dir() && recursive {
                dir_list.push(Arc::new(LinkedPath::new_child(&dir, entry.file_name())));
            } else if file_type.is_symlink() && follow_symlink {
                let entry_name = entry.file_name();
                let pop_token = push_to_path(&mut path_acc, &entry_name);
                let Ok(metadata) = std::fs::metadata(&pop_token.0) else { continue };
                let entry_name = LinkedPath::new_child(&dir, entry_name);
                if metadata.is_file() {
                    let keep_file = file_filter.keep_file_md(&entry_name, pop_token.0, &metadata);
                    if keep_file {
                        write_target(entry_name)
                    }
                } else if metadata.is_dir() && recursive {
                    dir_list.push(Arc::new(entry_name))
                }
            }
        }
    }
}

struct TemporarySegmentToken<'a>(&'a mut PathBuf);
impl <'a> Drop for TemporarySegmentToken<'a> {
    fn drop(&mut self) {
        self.0.pop();
    }
}
fn push_to_path<'a>(path: &'a mut PathBuf, segment: &OsString) -> TemporarySegmentToken<'a> {
    path.push(segment);
    TemporarySegmentToken(path)
}

fn place_into_file_set<'s, F>(
    file_path: LinkedPath,
    file: &PathBuf,
    tmp_buf: &mut PathBuf,
    refiners: &mut FileSetRefiners,
    find_set: F
) -> Result<(), AlreadyReportedError>
where F: FnOnce(u128) -> &'s mut Vec<(u128, Vec<HashedFile>)>{
    let hash = hash_file::<xxhash_rust::xxh3::Xxh3>(&file);
    let (mut hash, modtime) = match hash {
        Ok(value) => value,
        Err(HashFileError::FileChanged) => {
            handle_file_modified!(file);
            return Err(AlreadyReportedError)
        },
        Err(HashFileError::IO(err)) => {
            handle_file_error!(file, err);
            return Err(AlreadyReportedError)
        }
    };
    let file_hash = hash.digest128();
    refiners.hash_components(&mut hash, &file)?;

    let course_set = find_set(hash.digest128());

    if course_set.is_empty() {
        course_set.push((file_hash, vec![HashedFile { file_version_timestamp: modtime, file_path }]));
        return Ok(())
    }

    for (_, set) in course_set.iter_mut().filter(|(shash, _)| *shash == file_hash) {
        let fits = fits_into_file_set(set, file, tmp_buf, refiners)?;
        if fits {
            set.push(HashedFile { file_version_timestamp: modtime, file_path });
            break
        }
    }
    Ok(())
}

fn fits_into_file_set(file_set: &mut Vec<HashedFile>, file: &PathBuf, tmp_buf: &mut PathBuf, refiners: &mut FileSetRefiners) -> Result<bool, AlreadyReportedError>{
    loop {
        let Some(HashedFile { file_path: check_against, .. }) = file_set.first() else { return Ok(false) };
        check_against.write_full_to_buf(tmp_buf);

        let equals_result = refiners.check_equal(tmp_buf, file);

        match equals_result {
            Ok(is_eq) => return Ok(is_eq),
            Err(err) => {
                let (first_faulty, second_faulty) = err.is_faulty();
                if first_faulty {
                    file_set.remove(0);
                }
                if second_faulty {
                    return Err(AlreadyReportedError)
                }
            }
        }
    }
}

fn hash_file<H: Hasher + Default> (path: impl AsRef<Path>) -> Result<(H, Option<SystemTime>), HashFileError> {
    let mut hash = H::default();
    let mut file = std::fs::OpenOptions::new().read(true).write(false).open(path.as_ref())?;
    let metadata = file.metadata()?;
    let before_mod_time = metadata.modified().ok();// might be unavailable on the platform
    let mut buf = Box::new([0; 256]);
    hash_source(&mut buf, &mut hash, &mut file)?;
    let metadata = file.metadata()?;
    let after_mod_time = metadata.modified().ok();

    if before_mod_time == after_mod_time {
        Ok((hash, before_mod_time))
    } else {
        Err(HashFileError::FileChanged)
    }
}

fn hash_source<H: std::hash::Hasher>(buf: &mut Box<[u8; 256]>, hash: &mut H,mut file: impl std::io::Read) -> std::io::Result<()> {
    while let Some(bytes_read) = Some(file.read(buf.as_mut_slice())?).filter(|amount| *amount != 0) {
        hash.write(&buf[..bytes_read]);
    }
    Ok(())
}
