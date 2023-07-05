use std::alloc::Layout;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::hash::Hasher;
use std::io::{BufReader};
use std::mem::{ManuallyDrop, MaybeUninit, size_of_val};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use bumpalo::Bump;

#[derive(Clone, Debug)]
struct LinkedPath(Option<Arc<LinkedPath>>, OsString);

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
}

struct HashedFile {
    file_version_timestamp: Option<SystemTime>, file_path: LinkedPath
}

fn main()  {
    // shared-imm-state: Regex etc.
    // work queue: 1 Thread -> one working thread
    // single ui responsive

    // find file -> hash file -> lookup hash in hashmap -> equals check? -> confirm? -> needs accumulate(i.e. for sort)? -> execute action


    let find_files_time = Instant::now();
    let mut files= Vec::new();
    produce_list(true, |name| files.push(name));
    let find_files_time = find_files_time.elapsed();
    println!("found files: {find_files_time:?}");

    let mut path_buf = PathBuf::new();
    let mut target = HashMap::new();

    let hash_files_time = Instant::now();
    for file_path in files {
        path_buf.clear();
        file_path.push_full_to_buf(&mut path_buf);
        let file = std::fs::OpenOptions::new().read(true).write(false).open(&path_buf);
        let Ok(mut file) = file else { continue };
        let Ok(metadata) = file.metadata() else { continue };
        let before_mod_time = metadata.modified().ok();// might be unavailable on the platform
        let Ok(hash_value) = hash_file(&mut file) else { continue };
        let Ok(metadata) = file.metadata() else { continue };
        let after_mod_time = metadata.modified().ok();

        if before_mod_time == after_mod_time {
            target.entry(hash_value).or_insert(Vec::new()).push((after_mod_time, file_path));
        } else {
            //TODO log
        }
    }
    let hash_files_time = hash_files_time.elapsed();

    let delete_files_time = Instant::now();
    let mut sort_buf = Vec::new();
    for set in target.into_values() {
        if set.len() <= 1 {
            continue;
        }
        sort_buf.clear();

        let mut path_buf = PathBuf::new();
        for (last_modified, file_name) in set {
            path_buf.clear();
            file_name.push_full_to_buf(&mut path_buf);
            let Ok(file) = std::fs::OpenOptions::new().read(true).write(false).open(&path_buf) else { continue };
            let Ok(metadata) = file.metadata() else { continue };
            if metadata.modified().ok() != last_modified {
                continue;
            }
            let metadata = metadata.created().ok().unwrap();
            sort_buf.push((metadata, file_name))
        }

        sort_buf.sort_by_key(|(time, _)| *time);
        for (_, name) in &sort_buf[1..] {
            path_buf.clear();
            name.push_full_to_buf(&mut path_buf);
            println!("deleting {}", path_buf.display());
        }
    }
    let delete_files_time = delete_files_time.elapsed();
    println!("finding files: {:?}, hashing files: {:?}, deleting_files: {:?}", find_files_time, hash_files_time, delete_files_time)
}

fn produce_list(recursive: bool, mut write_target: impl FnMut(LinkedPath)) {
    let mut dir_list = vec![Arc::new(LinkedPath(None, OsString::from(".")))];

    let mut path_acc = PathBuf::new();
    while let Some(dir) = dir_list.pop() {
        path_acc.clear();
        dir.push_full_to_buf(&mut path_acc);
        let Ok(current_dir) = std::fs::read_dir(&path_acc) else { continue };
        for entry in current_dir {
            let Ok(entry) = entry else { break };
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_file() {
                write_target(LinkedPath(Some(dir.clone()), entry.file_name()));
            } else if file_type.is_dir() && recursive {
                dir_list.push(Arc::new(LinkedPath(Some(dir.clone()), entry.file_name())));
            }
        }
    }
}

fn hash_file(mut file: impl std::io::Read) -> std::io::Result<u128> {
    let mut buf = Box::new([0; 256]);
    let mut hash = xxhash_rust::xxh3::Xxh3::default();
    while let Some(bytes_read) = Some(file.read(buf.as_mut_slice())?).filter(|amount| *amount != 0) {
        hash.write(&buf[..bytes_read]);
    }
    Ok(hash.digest128())
}