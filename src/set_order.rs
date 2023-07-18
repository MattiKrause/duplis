use std::path::{PathBuf};
use std::time::SystemTime;
use crate::{handle_file_modified, handle_file_op, HashedFile};
use crate::error_handling::AlreadyReportedError;

pub trait SetOrder: DynCloneSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), AlreadyReportedError>;
}

crate::dyn_clone_impl!(DynCloneSetOrder, crate::set_order::SetOrder);

macro_rules! impl_new_rev {
    ($t: ident, $this: ident, $r: expr) => {
        impl $t {
            pub fn new(reverse: bool) -> Self {
                let mut $this = Self::default();
                $r.reverse = reverse;
                $this
            }
        }
    };
}

/// generic component for sorting from a metadata property
#[derive(Clone)]
pub struct MetadataSetOrder<F> { path_buf: PathBuf, file_buf: Vec<(F, HashedFile)>, reverse: bool }

/// don't sort at all
#[derive(Default, Clone)]
pub struct NoopSetOrder;
/// sort set by file modification timestamp
#[derive(Default, Clone)]
pub struct ModTimeSetOrder(MetadataSetOrder<SystemTime>);
/// sort set by file creation timestamp
#[derive(Default, Clone)]
pub struct CreateTimeSetOrder(MetadataSetOrder<SystemTime>);
#[derive(Default, Clone)]
pub struct SymlinkSetOrder(MetadataSetOrder<bool>);
/// sort set by file name
#[derive(Default, Clone)]
pub struct NameAlphabeticSetOrder { sort_buf: Vec<(HashedFile, PathBuf)>, unused_buf: Vec<PathBuf>, reverse: bool }


impl NoopSetOrder {
    pub fn new() -> Self { NoopSetOrder }
}

impl SetOrder for NoopSetOrder {
    fn order(&mut self, _files: &mut Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        Ok(())
    }
}

impl <T> Default for MetadataSetOrder<T> {
    fn default() -> Self {
        Self {
            path_buf: PathBuf::new(),
            file_buf: Vec::new(),
            reverse: false,
        }
    }
}

impl <F: Ord> MetadataSetOrder<F> {
    fn order(&mut self, files: &mut Vec<HashedFile>, key_extract: impl Fn(std::fs::Metadata) -> Result<F, AlreadyReportedError>) -> Result<(), AlreadyReportedError> {
        self.file_buf.clear();
        self.file_buf.reserve(files.len());
        for file_data in files.drain(..) {
            file_data.file_path.write_full_to_buf(&mut self.path_buf);

            // remove data from set if access error
            let metadata = handle_file_op!(self.path_buf.symlink_metadata(), self.path_buf, continue);

            let key = key_extract(metadata)?;
            self.file_buf.push((key, file_data))
        }
        // sort stable in case there are multiple sorters
        self.file_buf.sort_by(|(key1, _), (key2, _)| {
            let ordering = key1.cmp(key2);
            if self.reverse { ordering.reverse() } else { ordering }
        });
        files.extend(self.file_buf.drain(..).map(|(_, f)| f));
        Ok(())
    }
}

impl_new_rev!(ModTimeSetOrder, this, this.0);

impl SetOrder for ModTimeSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        self.0.order(files, |md| {
            md.modified().map_err(|err| {
                log::error!("cannot access modification time on current platform: {err}");
                AlreadyReportedError
            })
        })
    }
}
impl_new_rev!(CreateTimeSetOrder, this, this.0);

impl SetOrder for CreateTimeSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        self.0.order(files, |md| {
            md.created().map_err(|err| {
                log::error!("cannot access creation time on current platform: {err}");
                AlreadyReportedError
            })
        })
    }
}

impl SetOrder for SymlinkSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        self.0.order(files, |md| Ok(md.is_symlink()))
    }
}

impl_new_rev!(NameAlphabeticSetOrder, this, this);

impl SetOrder for NameAlphabeticSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), AlreadyReportedError> {
        self.sort_buf.clear();
        self.sort_buf.reserve(files.len());

        let files_with_names = files
            .drain(..)
            .zip(self.unused_buf.drain(..).chain(std::iter::repeat_with(PathBuf::new)))
            .map(|(file, mut name)| {
                file.file_path.write_full_to_buf(&mut name);
                (file, name)
            });

        self.sort_buf.extend(files_with_names);

        // sort stable in case we have multiple sorters
        self.sort_buf.sort_by(|(_, name1), (_, name2)| {
            let order = name1.cmp(name2);
            if self.reverse { order.reverse() } else { order }
        });
        self.unused_buf.reserve(self.sort_buf.len());
        for (file, name) in self.sort_buf.drain(..) {
            files.push(file);
            self.unused_buf.push(name);
        }
        Ok(())
    }
}