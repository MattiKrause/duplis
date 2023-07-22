use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[macro_export]
macro_rules! dyn_clone_impl {
    ($dcname: ident, $tname: path) => {
        pub trait $dcname {
            fn dyn_clone(&self) -> Box<dyn $tname + Send>;
        }

        impl <T: 'static + $tname + Clone + Send> $dcname for T {
            fn dyn_clone(&self) -> Box<dyn $tname + Send> {
                Box::new(self.clone())
            }
        }
    };
}

pub trait ChoiceInputReader {
    fn read_remaining(&mut self, buf: &mut String) -> std::io::Result<()>;
}

impl ChoiceInputReader for std::io::Stdin {
    fn read_remaining(&mut self, buf: &mut String) -> std::io::Result<()> {
        use std::io::BufRead;
        self.lock().read_line(buf).map(|_| ())
    }
}

#[derive(Clone, Debug)]
pub struct LinkedPath(Option<Arc<LinkedPath>>, OsString);
impl LinkedPath {
    pub fn new_child(parent: &Arc<LinkedPath>, segment: OsString) -> Self {
        Self(Some(parent.clone()), segment)
    }

    pub fn write_full_to_buf(&self, buf: &mut PathBuf) {
        buf.clear();
        self.push_full_to_buf(buf);
    }

    fn push_full_to_buf(&self, buf: &mut PathBuf) {
        if let Some(ancestor) = &self.0 {
            ancestor.push_full_to_buf(buf);
        }
        buf.push(&self.1);
    }

    pub fn to_push_buf(&self) -> PathBuf {
        let mut path_buf = PathBuf::new();
        self.push_full_to_buf(&mut path_buf);
        path_buf
    }

    pub fn from_path_buf(buf: &PathBuf) -> Arc<Self> {
        buf.iter().map(ToOwned::to_owned)
            .fold(None, |acc, res| Some(Arc::new(LinkedPath(acc, res))))
            .expect("empty path")
    }

    pub fn root(dir: &str) -> Arc<Self> {
        Arc::new(Self(None, OsString::from(dir)))
    }
}

pub fn path_contains_comma(path: &Path) -> bool {
    #[cfg(unix)]
    return {
        use std::os::unix::ffi::OsStrExt;;
        path.as_os_str().as_bytes().contains(&b',')
    };
    #[cfg(not(unix))]
    return {
        path.as_os_str().to_string_lossy().contains(b',')
    }
}
