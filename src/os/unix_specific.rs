use std::borrow::Cow;
use std::hash::Hasher;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use crate::file_set_refiner::{CheckEqualsError, FileEqualsChecker, FileWork};
use crate::os::{SetOrderOption, SimpleFieEqualCheckerArg, SimpleFileConsumeActionArg};
use crate::Recoverable;
use crate::set_consumer::{FileConsumeAction, FileConsumeResult};

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
        if let Err(err) = std::fs::remove_file(path) {
            match err.kind() {
                std::io::ErrorKind::NotFound => {}
                std::io::ErrorKind::PermissionDenied => {
                    log::info!("failed to delete file {} in order to replace it with a sym link due to lacking permissions", path.display());
                    return Err(Recoverable::Recoverable(()));
                }
                _ => {
                    log::warn!("failed ot delete file {} in order to replace it with a sym link due error {err}", path.display());
                }
            }
        };
        if let Err(err) = std::os::unix::fs::symlink(original, path) {
            log::error!("FATAL ERROR: failed to create sym link to {} from {} due to error {err}", path.display(), original.display());
            // Something is absolutely not right here, continuing means risk of data loss
            return Err(Recoverable::Fatal(()));
        }
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
        let a = std::fs::File::open(a).map_err(|_| CheckEqualsError::first_err())?;
        let b = std::fs::File::open(b).map_err(|_| CheckEqualsError::second_err())?;

        let metadata_a = a.metadata().map_err(|_| CheckEqualsError::first_err())?;
        let metadata_b= b.metadata().map_err(|_| CheckEqualsError::second_err())?;
        let perm_a = metadata_a.permissions().mode() & 0b111_111_111;
        let perm_b =metadata_b.permissions().mode() & 0b111_111_111;
        Ok(perm_a == perm_b)
    }

    fn hash_component(&mut self, a: &PathBuf, hasher: &mut dyn Hasher) -> Result<(), ()> {
        let perms = a.metadata().map_err(|_| ())?.mode() & 0b111_111_111;
        hasher.write_u32(perms);
        Ok(())
    }

    fn work_severity(&self) -> FileWork {
        FileWork::FileMetadataWork
    }
}