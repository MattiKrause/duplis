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
                return false
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
            return true
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
            let Ok(metadata) = entry.metadata() else { return false };
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