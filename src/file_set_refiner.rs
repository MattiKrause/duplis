use crate::error_handling::AlreadyReportedError;
use crate::{dyn_clone_impl, handle_file_op};
use std::io::Read;
use std::path::Path;

pub struct FileSetRefiners(Box<[Box<dyn FileEqualsChecker + Send>]>);

impl FileSetRefiners {
    pub fn new(mut checkers: Box<[Box<dyn FileEqualsChecker + Send>]>) -> Self {
        checkers.sort_unstable_by_key(|fec| fec.work_severity());
        Self(checkers)
    }

    pub fn hash_components(
        &mut self,
        hasher: &mut dyn std::hash::Hasher,
        file: &Path,
    ) -> Result<(), AlreadyReportedError> {
        for refiner in self.0.iter_mut() {
            refiner.hash_component(file, hasher)?;
        }
        Ok(())
    }

    pub fn check_equal(&mut self, a: &Path, b: &Path) -> Result<bool, CheckEqualsErrorOn> {
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
pub enum CheckEqualsErrorOn {
    First,
    Second,
    Both,
}

impl CheckEqualsErrorOn {
    pub fn is_faulty(self) -> (bool, bool) {
        match self {
            CheckEqualsErrorOn::First => (true, false),
            CheckEqualsErrorOn::Second => (false, true),
            CheckEqualsErrorOn::Both => (true, true),
        }
    }
    pub fn first_err() -> Self {
        Self::First
    }

    pub fn second_err() -> Self {
        Self::Second
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum FileWorkload {
    /// cpu bound work
    Simple = 0,
    /// compare based on some file property
    FileMetadata = 1,
    /// compare based on the file content itself
    FileContent = 2,
}

/// checks whether to files are equal
pub trait FileEqualsChecker: FileEqualsCheckDynClone {
    fn check_equal(&mut self, a: &Path, b: &Path) -> Result<bool, CheckEqualsErrorOn>;
    /// hash the property were checking for(like the permissions), may be a noop if property cannot be hashed.
    fn hash_component(
        &mut self,
        f: &Path,
        hasher: &mut dyn std::hash::Hasher,
    ) -> Result<(), AlreadyReportedError>;
    fn work_severity(&self) -> FileWorkload;
}

dyn_clone_impl!(FileEqualsCheckDynClone, FileEqualsChecker);

#[derive(Clone)]
pub struct FileContentEquals {
    buf: Box<([u8; 64], [u8; 64])>,
}

impl Default for FileContentEquals {
    fn default() -> Self {
        Self {
            buf: Box::new(([0; 64], [0; 64])),
        }
    }
}

impl FileContentEquals {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FileEqualsChecker for FileContentEquals {
    fn check_equal(&mut self, a_path: &Path, b_path: &Path) -> Result<bool, CheckEqualsErrorOn> {
        let (buf_a, buf_b) = &mut *self.buf;

        let mut a = handle_file_op!(
            std::fs::File::open(a_path),
            a_path,
            return Err(CheckEqualsErrorOn::First)
        );
        let mut b = handle_file_op!(
            std::fs::File::open(b_path),
            b_path,
            return Err(CheckEqualsErrorOn::Second)
        );

        let metadata_a =
            handle_file_op!(a.metadata(), a_path, return Err(CheckEqualsErrorOn::First));
        let metadata_b =
            handle_file_op!(b.metadata(), b_path, return Err(CheckEqualsErrorOn::Second));

        if metadata_a.len() != metadata_b.len() {
            return Ok(false);
        }

        loop {
            let l = handle_file_op!(a.read(buf_a), a_path, return Err(CheckEqualsErrorOn::First));
            if l == 0 {
                return Ok(true);
            }
            let l2 = handle_file_op!(
                b.read(buf_b),
                b_path,
                return Err(CheckEqualsErrorOn::Second)
            );
            if (l != l2) || (buf_a[..l] != buf_b[..l]) {
                return Ok(false);
            }
        }
    }

    fn hash_component(
        &mut self,
        _a: &Path,
        _hasher: &mut dyn std::hash::Hasher,
    ) -> Result<(), AlreadyReportedError> {
        Ok(())
    }

    fn work_severity(&self) -> FileWorkload {
        FileWorkload::FileContent
    }
}
