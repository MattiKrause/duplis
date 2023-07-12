use std::path::PathBuf;
use std::time::SystemTime;
use crate::{BoxErr, HashedFile};
use crate::util::DynCloneSetOrder;

pub trait SetOrder: DynCloneSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), BoxErr>;
}

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

#[derive(Clone)]
pub struct MetadataSetOrder<F> { path_buf: PathBuf, file_buf: Vec<(F, HashedFile)>, reverse: bool }

#[derive(Default, Clone)]
pub struct NoopSetOrder;
#[derive(Default, Clone)]
pub struct ModTimeSetOrder(MetadataSetOrder<SystemTime>);
#[derive(Default, Clone)]
pub struct CreateTimeSetOrder(MetadataSetOrder<SystemTime>);
#[derive(Default, Clone)]
pub struct NameAlphabeticSetOrder { sort_buf: Vec<(HashedFile, PathBuf)>, unused_buf: Vec<PathBuf>, reverse: bool }

impl NoopSetOrder {
    pub fn new() -> Self { NoopSetOrder }
}

impl SetOrder for NoopSetOrder {
    fn order(&mut self, _files: &mut Vec<HashedFile>) -> Result<(), Box<dyn std::error::Error>> {
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
    fn order(&mut self, files: &mut Vec<HashedFile>, key_extract: impl Fn(std::fs::Metadata) -> Result<F, BoxErr>) -> Result<(), BoxErr> {
        self.file_buf.clear();
        self.file_buf.reserve(files.len());
        for file_data in files.drain(..) {
            self.path_buf.clear();
            file_data.file_path.push_full_to_buf(&mut self.path_buf);

            let file = std::fs::OpenOptions::new().read(true).write(false)
                .open(&self.path_buf);
            let Ok(file) = file else { continue };
            let Ok(metadata) = file.metadata() else { continue };
            if metadata.modified().ok() != file_data.file_version_timestamp {
                continue;
            }

            let key = key_extract(metadata)?;
            self.file_buf.push((key, file_data))
        }
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
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), BoxErr> {
        self.0.order(files, |md| md.modified().map_err(|err| BoxErr::from(format!("cannot access modification time on current platform: {err}"))))
    }
}
impl_new_rev!(CreateTimeSetOrder, this, this.0);

impl SetOrder for CreateTimeSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), BoxErr> {
        self.0.order(files, |md| md.created().map_err(|err| format!("cannot access modification time on current platform: {err}").into()))
    }
}

impl_new_rev!(NameAlphabeticSetOrder, this, this);

impl SetOrder for NameAlphabeticSetOrder {
    fn order(&mut self, files: &mut Vec<HashedFile>) -> Result<(), BoxErr> {
        self.sort_buf.clear();
        self.sort_buf.reserve(files.len());

        let files_with_names = files
            .drain(..)
            .zip(self.unused_buf.drain(..).chain(std::iter::repeat_with(PathBuf::new)))
            .map(|(file, mut name)| {
                name.clear();
                file.file_path.push_full_to_buf(&mut name);
                (file, name)
            });

        self.sort_buf.extend(files_with_names);
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