use std::borrow::Cow;
use std::path::Path;
use crate::Recoverable;
use crate::set_consumer::{FileConsumeAction, FileConsumeResult};

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

