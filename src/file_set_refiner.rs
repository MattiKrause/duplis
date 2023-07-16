use std::io::Read;
use std::ops::DerefMut;
use std::path::PathBuf;
use crate::error_handling::AlreadyReportedError;
use crate::handle_file_op;

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
    SimpleWork = 0, FileMetadataWork = 1, FileContentWork = 2
}

pub trait FileEqualsChecker {
    fn check_equal(&mut self, a: &PathBuf, b: &PathBuf) -> Result<bool, CheckEqualsError>;
    fn hash_component(&mut self, f: &PathBuf, hasher:  &mut dyn std::hash::Hasher) -> Result<(), AlreadyReportedError>;
    fn work_severity(&self) -> FileWork;
}

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
    fn check_equal(&mut self, a_path: &PathBuf, b_path: &PathBuf) -> Result<bool, CheckEqualsError>{
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

    fn hash_component(&mut self, _a: &PathBuf, _hasher: &mut dyn std::hash::Hasher) -> Result<(), AlreadyReportedError>{ Ok(()) }

    fn work_severity(&self) -> FileWork {
        FileWork::FileContentWork
    }
}