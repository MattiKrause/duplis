mod set_order;
mod set_consumer;
mod parse_cli;
mod os;
mod file_set_refiner;
mod util;
mod file_filters;
mod error_handling;


use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::{Hasher};


use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use clap::builder::ValueRange;
use log::LevelFilter;
use simplelog::{Config, ConfigBuilder};
use crate::error_handling::AlreadyReportedError;
use crate::file_filters::FileFilter;
use crate::file_set_refiner::FileEqualsChecker;


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
    let mut set_refiners: Vec<Box<dyn FileEqualsChecker>> = file_equals;
    set_refiners.sort_unstable_by_key(|fec| fec.work_severity());
    let mut target: HashMap<u128, (Vec<(u128, Vec<HashedFile>)>)> = HashMap::new();


    for file_path in files_rev.into_iter() {
        path_buf.clear();
        file_path.push_full_to_buf(&mut path_buf);
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

fn produce_list(path: Arc<LinkedPath>, mut file_filter: &mut FileFilter, recursive: bool, follow_symlink: bool, mut write_target: impl FnMut(LinkedPath)) {
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
                let file_name = entry.file_name();
                path_acc.push(file_name);
                let file_name = LinkedPath(Some(dir.clone()), entry.file_name());
                let keep_file = if cfg!(windows) {
                    if let Ok(md) = entry.metadata() {
                        file_filter.keep_file_md(&file_name, &path_acc, &md)
                    } else {
                        false
                    }
                } else {
                    file_filter.keep_file(&file_name, &path_acc)
                };
                if keep_file {
                    write_target(file_name);
                }
                path_acc.pop();
            } else if file_type.is_dir() && recursive {
                dir_list.push(Arc::new(LinkedPath(Some(dir.clone()), entry.file_name())));
            } else if file_type.is_symlink() && follow_symlink {
                let entry_name = entry.file_name();
                path_acc.push(&entry_name);
                let Ok(metadata) = std::fs::metadata(&path_acc) else {
                    path_acc.pop();
                    continue
                };
                let entry_name = LinkedPath(Some(dir.clone()), entry_name);
                if metadata.is_file() {
                    let keep_file = file_filter.keep_file_md(&entry_name, &path_acc, &metadata);
                    if keep_file {
                        write_target(entry_name)
                    }
                } else if metadata.is_dir() && recursive {
                    dir_list.push(Arc::new(entry_name))
                }
                path_acc.pop();
            }
        }
    }
}

fn place_into_file_set<'s, F>(
    file_path: LinkedPath,
    file: &PathBuf,
    tmp_buf: &mut PathBuf,
    refiners: &mut [Box<dyn FileEqualsChecker>],
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
    for mut refiner in refiners.iter_mut() {
        refiner.hash_component(&file, &mut hash)?;
    }
    let course_set = find_set(hash.digest128());
    if course_set.is_empty() {
        course_set.push((file_hash, vec![HashedFile { file_version_timestamp: modtime, file_path }]));
        return Ok(())
    }
    
    'set_loop:
    for (_, set) in course_set.iter_mut().filter(|(shash, _)| *shash == file_hash) {
        tmp_buf.clear();
        let check_against = if let Some(HashedFile { file_path, .. }) = set.first() {
            file_path
        } else {
            set.push(HashedFile { file_version_timestamp: modtime, file_path });
            break;
        };
        check_against.push_full_to_buf(tmp_buf);
        
        
        for refiner in refiners.iter_mut() {
            let is_equals = match refiner.check_equal(tmp_buf, file) {
                Ok(result) => result,
                Err(err) => {
                    let (first_faulty, second_faulty) = err.is_faulty();
                    if first_faulty {
                       set.remove(0);
                        break;//TODO replace with retry
                    }
                    if second_faulty {
                        return Err(AlreadyReportedError)
                    }
                    continue;
                }
            };
            if !is_equals {
                continue 'set_loop;
            }
        }

        set.push(HashedFile { file_version_timestamp: modtime, file_path });
        break
    }
    Ok(())
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
