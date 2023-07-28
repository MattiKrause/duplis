use std::io::Read;
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use crate::error_handling::AlreadyReportedError;
use crate::{dyn_clone_impl, handle_file_op};

pub struct FileSetRefiners(Box<[Box<dyn FileEqualsChecker + Send>]>);

impl FileSetRefiners {
    pub fn new(mut checkers: Box<[Box<dyn FileEqualsChecker + Send>]>) -> Self {
        checkers.sort_unstable_by_key(|fec| fec.work_severity());
        Self(checkers)
    }

    pub fn hash_components(&mut self, hasher: &mut dyn std::hash::Hasher, file: &PathBuf) -> Result<(), AlreadyReportedError> {
        for refiner in self.0.iter_mut() {
            refiner.hash_component(file, hasher)?;
        }
        Ok(())
    }

    pub fn check_equal(&mut self, a: &PathBuf, b: &PathBuf) -> Result<bool, CheckEqualsError> {
        for refiner in self.0.iter_mut() {
            refiner.check_equal(a, b)?;
        }
        Ok(true)
    }
}

impl Clone for FileSetRefiners {
    fn clone(&self) -> Self {
        let cks = self.0.iter().map(|ck| ck.dyn_clone()).collect::<Vec<_>>();
        Self(cks.into_boxed_slice())
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CheckEqualsError {
    FirstFaulty, SecondFaulty, BothFaulty
}

impl CheckEqualsError {
    pub fn is_faulty(&self) -> (bool, bool) {
        match self {
            CheckEqualsError::FirstFaulty => (true, false),
            CheckEqualsError::SecondFaulty => (false, true),
            CheckEqualsError::BothFaulty => (true, true)
        }
    }
    pub fn first_err() -> Self {
        Self::FirstFaulty
    }

    pub fn second_err() -> Self {
        Self::SecondFaulty
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum FileWork {
    /// cpu bound work
    SimpleWork = 0,
    /// compare based on some file property
    FileMetadataWork = 1,
    /// compare based on the file content itself
    FileContentWork = 2
}

/// checks whether to files are equal
pub trait FileEqualsChecker: FileEqualsCheckDynClone {
    fn check_equal(&mut self, a: &Path, b: &Path) -> Result<bool, CheckEqualsError>;
    /// hash the property were checking for(like the permissions), may be a noop if property cannot be hashed.
    fn hash_component(&mut self, f: &Path, hasher:  &mut dyn std::hash::Hasher) -> Result<(), AlreadyReportedError>;
    fn work_severity(&self) -> FileWork;
}

dyn_clone_impl!(FileEqualsCheckDynClone, FileEqualsChecker);

#[derive(Clone)]
pub struct FileContentEquals {
   buf: Box<([u8; 64], [u8; 64])>
}

impl Default for FileContentEquals {
    fn default() -> Self {
        Self { buf: Box::new(([0; 64], [0; 64])) }
    }
}

impl FileContentEquals {
    pub fn new() -> Self { Self::default() }
}

impl FileEqualsChecker for FileContentEquals{
    fn check_equal(&mut self, a_path: &Path, b_path: &Path) -> Result<bool, CheckEqualsError>{
        let (buf_a, buf_b) = self.buf.deref_mut();

        let mut a = handle_file_op!(std::fs::File::open(a_path), a_path, return Err(CheckEqualsError::FirstFaulty));
        let mut b = handle_file_op!(std::fs::File::open(b_path), b_path, return Err(CheckEqualsError::SecondFaulty));

        let metadata_a = handle_file_op!(a.metadata(), a_path, return Err(CheckEqualsError::FirstFaulty));
        let metadata_b= handle_file_op!(b.metadata(), b_path, return Err(CheckEqualsError::SecondFaulty));

        if metadata_a.len() != metadata_b.len() {
            return Ok(false)
        }

        loop {
            let l = handle_file_op!(a.read(buf_a), a_path, return Err(CheckEqualsError::FirstFaulty));
            if l == 0 {
                return Ok(true);
            }
            handle_file_op!(b.read(buf_b), b_path, return Err(CheckEqualsError::SecondFaulty));
            if buf_a[..l] != buf_b[..l] {
                return Ok(false);
            }
        }
    }

    fn hash_component(&mut self, _a: &Path, _hasher: &mut dyn std::hash::Hasher) -> Result<(), AlreadyReportedError>{ Ok(()) }

    fn work_severity(&self) -> FileWork {
        FileWork::FileContentWork
    }
}