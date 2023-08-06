#![warn(clippy::pedantic)]
#![allow(clippy::redundant_else, clippy::module_name_repetitions)]

extern crate core;

mod set_order;
mod set_consumer;
mod parse_cli;
mod os;
mod file_set_refiner;
mod util;
mod file_filters;
mod error_handling;
mod file_action;
#[cfg(test)]
mod common_tests;
mod logger;
mod input_source;

use std::io::stderr;
use std::ops::DerefMut;


use std::path::{Path, PathBuf};
use std::time::SystemTime;
use dashmap::DashMap;

use log::{LevelFilter};
use crate::error_handling::AlreadyReportedError;
use crate::file_set_refiner::{FileSetRefiners};
use crate::input_source::{ChannelInputSink, DedupingInputSink, InputSink};


use crate::parse_cli::ExecutionPlan;
use crate::set_order::SymlinkSetOrder;
use crate::util::LinkedPath;

pub enum Recoverable<R, F> {
    Recoverable(R),
    Fatal(F),
}


enum HashFileError {
    IO(std::io::Error),
    FileChanged,
}

impl From<std::io::Error> for HashFileError {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HashedFile {
    file_version_timestamp: Option<SystemTime>,
    file_path: LinkedPath,
}

pub type BoxErr = Box<dyn std::error::Error>;

fn main() {
    // the data required to run the program
    let ExecutionPlan { file_equals, mut order_set, action: mut file_set_action, num_threads, ignore_log_set, input_sources, dedup_files } = parse_cli::parse();

    logger::DuplisLogger::init(ignore_log_set, LevelFilter::Trace, Box::new(stderr())).unwrap();

    let set_refiners = FileSetRefiners::new(file_equals.into_boxed_slice());
    order_set.push(Box::<SymlinkSetOrder>::default());
    // if don't thread we want essentially a list, if we thread, there is no harm in keeping then backlog in check
    let (files_send, files_rev): (flume::Sender<LinkedPath>, _) = if num_threads.get() > 1 { flume::bounded(128) } else { flume::unbounded() };
    let target: DashMap<u128, Vec<(u128, Vec<HashedFile>)>> = DashMap::new();

    std::thread::scope(|s| {
        if num_threads.get() > 1 {
            // spawn n - 1 threads, if is for clarity
            for t in 1..num_threads.get() {
                let set_refiners = set_refiners.clone();
                let files_rev = files_rev.clone();
                let thread = std::thread::Builder::new()
                    .name(format!("file_hash_worker_{t}"))
                    .spawn_scoped(s, || place_files_to_set(set_refiners, files_rev, &target));
                if let Err(err) = thread {
                    log::error!(target: crate::error_handling::CONFIG_ERR_TARGET, "threading not supported on this platform; please do not use the threading option({err})");
                    return;
                }
            }
        }
        let mut input_sink: Box<dyn InputSink + Send> = Box::new(ChannelInputSink::new(files_send));
        if dedup_files {
            input_sink = Box::new(DedupingInputSink::new(input_sink));
        }
        for mut source in input_sources {
            let _ = source.consume_all(input_sink.as_mut());
        }

        drop(input_sink);

        if num_threads.get() == 1 {
            place_files_to_set(set_refiners, files_rev, &target);
        }
    });
    for mut set in target.into_iter().map(|(_, v)| v).flat_map(std::iter::IntoIterator::into_iter) {
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

        if file_set_action.consume_set(set.1).is_err() {
            break;
        };
    }
}

fn place_files_to_set(mut set_refiners: FileSetRefiners, files: flume::Receiver<LinkedPath>, target: &DashMap<u128, Vec<(u128, Vec<HashedFile>)>>) {
    let mut path_buf = PathBuf::new();
    let mut path_buf_tmp = PathBuf::new();

    for file_path in files {
        file_path.write_full_to_buf(&mut path_buf);
        let _ = place_into_file_set(file_path, &path_buf, &mut path_buf_tmp, &mut set_refiners, |hash| target.entry(hash).or_insert(Vec::new()));
    }
}

fn place_into_file_set<R, F>(
    file_path: LinkedPath,
    file: &Path,
    tmp_buf: &mut PathBuf,
    refiners: &mut FileSetRefiners,
    find_set: F,
) -> Result<(), AlreadyReportedError>
    where R: DerefMut<Target=Vec<(u128, Vec<HashedFile>)>>, F: FnOnce(u128) -> R {
    let hash = hash_file::<xxhash_rust::xxh3::Xxh3>(&file);
    let (mut hash, modtime) = match hash {
        Ok(value) => value,
        Err(HashFileError::FileChanged) => {
            handle_file_modified!(file);
            return Err(AlreadyReportedError);
        }
        Err(HashFileError::IO(err)) => {
            handle_file_error!(file, err);
            return Err(AlreadyReportedError);
        }
    };
    let file_hash = hash.digest128();
    refiners.hash_components(&mut hash, file)?;

    let mut course_set = find_set(hash.digest128());
    let course_set = &mut *course_set;

    // we have created an new course set, thus there is nothing to compare this file to
    if course_set.is_empty() {
        course_set.push((file_hash, vec![HashedFile { file_version_timestamp: modtime, file_path }]));
        return Ok(());
    }

    for (_, set) in course_set.iter_mut().filter(|(shash, _)| *shash == file_hash) {
        let fits = fits_into_file_set(set, file, tmp_buf, refiners)?;
        if fits {
            set.push(HashedFile { file_version_timestamp: modtime, file_path });
            break;
        }
    }
    Ok(())
}

fn fits_into_file_set(file_set: &mut Vec<HashedFile>, file: &Path, tmp_buf: &mut PathBuf, refiners: &mut FileSetRefiners) -> Result<bool, AlreadyReportedError> {
    loop {
        let Some(HashedFile { file_path: check_against, .. }) = file_set.first() else { return Ok(false); };
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
                    return Err(AlreadyReportedError);
                }
            }
        }
    }
}

fn hash_file<H: std::hash::Hasher + Default>(path: impl AsRef<Path>) -> Result<(H, Option<SystemTime>), HashFileError> {
    let mut hash = H::default();
    let mut file = std::fs::OpenOptions::new().read(true).write(false).open(path.as_ref())?;
    let metadata = file.metadata()?;
    let before_mod_time = metadata.modified().ok();// might be unavailable on the platform
    let mut buf = Box::new([0; 512]);
    hash_source(&mut buf, &mut hash, &mut file)?;
    let metadata = file.metadata()?;
    let after_mod_time = metadata.modified().ok();

    if before_mod_time == after_mod_time {
        Ok((hash, before_mod_time))
    } else {
        Err(HashFileError::FileChanged)
    }
}

fn hash_source<H: std::hash::Hasher>(buf: &mut Box<[u8; 512]>, hash: &mut H, mut file: impl std::io::Read) -> std::io::Result<()> {
    while let Some(bytes_read) = Some(file.read(buf.as_mut_slice())?).filter(|amount| *amount != 0) {
        hash.write(&buf[..bytes_read]);
    }
    Ok(())
}
