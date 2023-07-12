use std::hash::Hasher;
use std::io::Read;
use std::ops::DerefMut;
use std::path::PathBuf;

pub struct CheckEqualsError {
    pub first_faulty: bool, pub second_faulty: bool
}

impl CheckEqualsError {
    pub fn first_err() -> Self {
        Self { first_faulty: true, second_faulty: false }
    }

    pub fn second_err() -> Self {
        Self { first_faulty: false, second_faulty: true }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum FileWork {
    SimpleWork = 0, FileMetadataWork = 1, FileContentWork = 2
}

pub trait FileEqualsChecker {
    fn check_equal(&mut self, a: &PathBuf, b: &PathBuf) -> Result<bool, CheckEqualsError>;
    fn hash_component(&mut self, f: &PathBuf, hasher:  &mut dyn std::hash::Hasher) -> Result<(), ()>;
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
    fn check_equal(&mut self, a: &PathBuf, b: &PathBuf) -> Result<bool, CheckEqualsError>{
        let (buf_a, buf_b) = self.buf.deref_mut();

        let mut a = std::fs::File::open(a).map_err(|_| CheckEqualsError::first_err())?;
        let mut b = std::fs::File::open(b).map_err(|_| CheckEqualsError::second_err())?;

        let metadata_a = a.metadata().map_err(|_| CheckEqualsError::first_err())?;
        let metadata_b= b.metadata().map_err(|_| CheckEqualsError::second_err())?;

        if metadata_a.len() != metadata_b.len() {
            return Ok(false)
        }

        loop {
            let l = a.read(buf_a).map_err(|_| CheckEqualsError::first_err())?;
            if l == 0 {
                return Ok(true);
            }
            b.read(buf_b).map_err(|_| CheckEqualsError::second_err())?;
            if buf_a[..l] != buf_b[..l] {
                return Ok(false);
            }
        }
    }

    fn hash_component(&mut self, _a: &PathBuf, _hasher: &mut dyn Hasher) -> Result<(), ()>{ Ok(()) }

    fn work_severity(&self) -> FileWork {
        FileWork::FileContentWork
    }
}