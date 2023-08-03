use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::Metadata;
use std::ops::DerefMut;
use std::path::Path;
use crate::{handle_file_op};
use crate::util::LinkedPath;

pub struct FileFilter(pub Box<[Box<dyn FileNameFilter>]>, pub Box<[Box<dyn FileMetadataFilter>]>);

impl FileFilter {
    fn filter_name(&mut self, name: &LinkedPath, name_path: &Path) -> bool {
        for name_filter in self.0.deref_mut() {
            let result = name_filter.filter_file_name(name, name_path).unwrap_or(false);
            if !result {
                return false;
            }
        }
        true
    }

    fn filter_metadata(&mut self, name: &LinkedPath, name_path: &Path, metadata: &Metadata) -> bool {
        for metadata_filter in self.1.deref_mut() {
            let result = metadata_filter.filter_file_metadata(name, name_path, &metadata).unwrap_or(false);
            if !result {
                return false;
            }
        }

        true
    }

    /// run the file through all filters, request metadata as needed,  return true if all filters return true
    pub fn keep_file(&mut self, name: &LinkedPath, name_path: &Path) -> bool {
        if !self.filter_name(name, name_path) {
            return false;
        }
        if self.1.is_empty() {
            return true;
        }
        let metadata = handle_file_op!(std::fs::metadata(name_path), name_path, return false);
        self.filter_metadata(name, name_path, &metadata)
    }

    /// run the file through all filters with the metadata provided
    pub fn keep_file_md(&mut self, name: &LinkedPath, name_path: &Path, metadata: &Metadata) -> bool {
        if !self.filter_name(name, name_path) {
            return false;
        }
        self.filter_metadata(name, name_path, metadata)
    }

    pub fn keep_file_dir_entry(&mut self, name: &LinkedPath, name_path: &Path, entry: std::fs::DirEntry) -> bool {
        if cfg!(windows) {
            let Ok(metadata) = entry.metadata() else { return false; };
            self.keep_file_md(&name, &name_path, &metadata)
        } else {
            self.keep_file(name, name_path)
        }
    }
}

/// Filters files only based on the name
pub trait FileNameFilter {
    fn filter_file_name(&mut self, name: &LinkedPath, name_path: &Path) -> Result<bool, ()>;
}

/// Filters files based on the name and metadata
pub trait FileMetadataFilter {
    fn filter_file_metadata(&mut self, name: &LinkedPath, name_path: &Path, metadata: &Metadata) -> Result<bool, ()>;
}

/// Only allow files with more than the given size
pub struct MinSizeFileFilter(u64);

/// Only allow files with less than the given size
pub struct MaxSizeFileFilter(u64);

/// Only allow files whose extensions are not in the set
pub struct ExtensionFilter {
    extensions: HashSet<OsString>,
    no_ext_in_set: bool,
    /// if true then extensions is a white-list, otherwise, extensions is a blacklist
    positive: bool,
}

//FIXME/TODO what happens to discovered symlinks, are they canonicalised, how should symlink filters be treated?
pub struct PathFilter(PathFilterTree);

#[derive(Debug)]
struct PathFilterTree(HashMap<OsString, Option<PathFilterTree>>);

impl MinSizeFileFilter {
    pub fn new(min: u64) -> Self {
        Self(min)
    }
}

impl FileMetadataFilter for MinSizeFileFilter {
    fn filter_file_metadata(&mut self, _: &LinkedPath, _: &Path, metadata: &Metadata) -> Result<bool, ()> {
        Ok(metadata.len() > self.0)
    }
}

impl MaxSizeFileFilter {
    pub(crate) fn new(max: u64) -> Self {
        Self(max)
    }
}

impl FileMetadataFilter for MaxSizeFileFilter {
    fn filter_file_metadata(&mut self, _: &LinkedPath, _: &Path, metadata: &Metadata) -> Result<bool, ()> {
        Ok(metadata.len() < self.0)
    }
}

impl ExtensionFilter {
    pub(crate) fn new(extensions: HashSet<OsString>, no_extension_in_set: bool, positive: bool) -> Self {
        Self { extensions, no_ext_in_set: no_extension_in_set, positive }
    }
}

impl FileNameFilter for ExtensionFilter {
    fn filter_file_name(&mut self, _: &LinkedPath, name_path: &Path) -> Result<bool, ()> {
        Ok(name_path.extension().map_or(self.no_ext_in_set, |ext| self.extensions.contains(ext)) ^ !self.positive)
    }
}

impl PathFilter {
    pub(crate) fn new<'p>(paths: impl Iterator<Item=&'p Path>) -> Self {
        let mut root = PathFilterTree(HashMap::new());
        'path_loop:
        for path in paths {
            let mut current = &mut root;
            if let Some(parent) = path.parent() {

                for seg in parent.iter() {
                    if current.0.contains_key(seg) {
                        let entry = current.0.get_mut(seg).unwrap().as_mut();
                        if let Some(next) = entry {
                            current = next;
                        } else {
                            // the entry contains none which means, that the whole path from here is blocked
                            continue 'path_loop;
                        }
                    } else {
                        current = current.0.entry(seg.to_os_string()).or_insert(Some(PathFilterTree(HashMap::new())))
                            .as_mut()
                            .unwrap()
                    }
                }
            }
            let Some(file_name) = path.file_name() else { continue; };
            current.0.insert(file_name.to_os_string(), None);
        }
        Self(root)
    }
}

impl FileNameFilter for PathFilter {
    fn filter_file_name(&mut self, _: &LinkedPath, name_path: &Path) -> Result<bool, ()> {
        let mut current = &self.0;
        for seg in name_path.iter() {
            let Some(entry) = current.0.get(seg) else { return Ok(true); };
            match entry.as_ref() {
                Some(next) => current = next,
                None => return Ok(false)
            }
        }
        Ok(true)
    }
}