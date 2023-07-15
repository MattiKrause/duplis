use std::fs::Metadata;
use std::ops::DerefMut;
use std::path::Path;
use crate::LinkedPath;

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

    pub fn keep_file(&mut self, name: &LinkedPath, name_path: &Path) -> bool {
        if !self.filter_name(name, name_path) {
            return false;
        }
        let metadata = match std::fs::metadata(name_path) {
            Ok(metadata) => metadata,
            Err(err) => {
                dbg!(name_path);
                println!("{err}");
                return false
            },
        };
        self.filter_metadata(name, name_path, &metadata)
    }

    pub fn keep_file_md(&mut self, name: &LinkedPath, name_path: &Path, metadata: &Metadata) -> bool {
        if !self.filter_name(name, name_path) {
            return false;
        }
        self.filter_metadata(name, name_path, metadata)
    }
}

pub trait FileNameFilter {
    fn filter_file_name(&mut self, name: &LinkedPath, name_path: &Path) -> Result<bool, ()>;
}

pub trait FileMetadataFilter {
    fn filter_file_metadata(&mut self, name: &LinkedPath, name_path: &Path, metadata: &Metadata) -> Result<bool, ()>;
}

pub struct MinSizeFileFilter(pub u64);
pub struct MaxSizeFileFilter(pub u64);

impl FileMetadataFilter for MinSizeFileFilter {
    fn filter_file_metadata(&mut self, _: &LinkedPath, _: &Path, metadata: &Metadata) -> Result<bool, ()> {
        Ok(metadata.len() > self.0)
    }
}

impl FileMetadataFilter for MaxSizeFileFilter {
    fn filter_file_metadata(&mut self, _: &LinkedPath, _: &Path, metadata: &Metadata) -> Result<bool, ()> {
        Ok(metadata.len() < self.0)
    }
}