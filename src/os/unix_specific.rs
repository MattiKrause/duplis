use std::borrow::Cow;
use std::hash::Hasher;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use crate::file_set_refiner::{CheckEqualsError, FileEqualsChecker, FileWork};
use crate::os::{SetOrderOption, SimpleFieEqualCheckerArg, SimpleFileConsumeActionArg};
use crate::{handle_file_op, Recoverable, report_file_action};
use crate::error_handling::AlreadyReportedError;
use crate::file_action::{FileConsumeAction, FileConsumeResult};

pub fn get_set_order_options() -> Vec<SetOrderOption> {
    vec![]
}

pub fn get_file_consume_action_simple() -> Vec<SimpleFileConsumeActionArg> {
    let rsymlink = SimpleFileConsumeActionArg {
        name: "resl",
        short: 'L',
        long: "resymlink",
        help: String::from("replace duplicate files with a symlink"),
        action: Box::new(ReplaceWithSymlinkFileAction),
    };
    vec![rsymlink]
}

pub fn get_file_equals_arg_simple() -> Vec<SimpleFieEqualCheckerArg> {
    let perm_eq = SimpleFieEqualCheckerArg {
        name: "perm_eq",
        short: 'p',
        long: "permeq",
        help: String::from("consider files with different permissions different files"),
        action: Box::new(PermissionEqualChecker),
    };

    vec![perm_eq]
}

pub struct ReplaceWithSymlinkFileAction;

impl FileConsumeAction for ReplaceWithSymlinkFileAction {
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult {
        let original = original.expect("original required");
        let original = handle_file_op!(std::fs::canonicalize(original), original, return Err(Recoverable::Recoverable(AlreadyReportedError)));
        handle_file_op!(std::fs::remove_file(path), path, return Err(Recoverable::Recoverable(AlreadyReportedError)));
        if let Err(err) = std::os::unix::fs::symlink(&original, path) {
            log::error!("FATAL ERROR: failed to create sym link to {} from {} due to error {err}", path.display(), original.display());
            // Something is absolutely not right here, continuing means risk of data loss
            return Err(Recoverable::Fatal(AlreadyReportedError));
        }
        report_file_action!("replaced {} with symlink to {}", path.display(), original.display());
        Ok(())
    }

    fn requires_original(&self) -> bool {
        true
    }

    fn short_name(&self) -> Cow<str> {
        Cow::Borrowed("replace with symlink")
    }

    fn short_opposite(&self) -> Cow<str> {
        Cow::Borrowed("keep")
    }
}

#[derive(Clone, Default)]
struct PermissionEqualChecker;

impl FileEqualsChecker for PermissionEqualChecker {
    fn check_equal(&mut self, a: &PathBuf, b: &PathBuf) -> Result<bool, CheckEqualsError> {
        let metadata_a = handle_file_op!(a.metadata(), a, return Err(CheckEqualsError::first_err()));
        let metadata_b= handle_file_op!(b.metadata(), b, return Err(CheckEqualsError::second_err()));
        let perm_a = metadata_a.permissions().mode() & 0b111_111_111;
        let perm_b =metadata_b.permissions().mode() & 0b111_111_111;
        Ok(perm_a == perm_b)
    }

    fn hash_component(&mut self, a: &PathBuf, hasher: &mut dyn Hasher) -> Result<(), AlreadyReportedError> {
        let metadata = handle_file_op!(a.metadata(), a, return Err(AlreadyReportedError));
        let perms = metadata.mode() & 0b111_111_111;
        hasher.write_u32(perms);
        Ok(())
    }

    fn work_severity(&self) -> FileWork {
        FileWork::FileMetadataWork
    }
}