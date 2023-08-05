use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use crate::error_handling::AlreadyReportedError;
use crate::file_filters::FileFilter;
use crate::util::{LinkedPath, push_to_path};

pub struct InputSink(flume::Sender<LinkedPath>);

impl InputSink {
    pub fn new(sink: flume::Sender<LinkedPath>) -> Self {
        Self(sink)
    }
    pub fn put(&mut self, path: LinkedPath) {
        if let Err(path) = self.0.send(path) {
            log::warn!(target: crate::error_handling::DISCOVERY_ERR_TARGET, "path sink closed! dropping path {}", path.0.to_push_buf().display())
        };
    }
}

pub trait InputSource {
    fn consume_all(&mut self, sink: &mut InputSink) -> Result<(), AlreadyReportedError>;
}

pub struct DiscoveringInputSource {
    recurse: bool,
    follow_symlink: bool,
    file_filters: FileFilter,
    sources: Vec<Arc<LinkedPath>>,
    path_acc: PathBuf,
}

pub struct StdInSource {
    file_filters: FileFilter
}

macro_rules! handle_access_dir {
    ($result: expr, $dir: expr, $action: expr) => {
        match $result {
            Ok(dir) => dir,
            Err(err) => {
                log::trace!(target: crate::error_handling::DISCOVERY_ERR_TARGET, "failed to access directory {}: {err}", $dir.display());
                $action
            }
        }
    };
}

macro_rules! handle_get_file_type {
    ($result: expr, $path: expr, $file_name: expr, $on_err: expr) => {{
        match $result {
            Ok(ft) => ft,
            Err(err) => {
                if log::log_enabled!(log::Level::Trace) {
                    $path.push($file_name);
                    log::trace!(target: crate::error_handling::DISCOVERY_ERR_TARGET, "failed to access {}: {err}", $path.display());
                    $path.pop();
                }
                $on_err
            }
        }
    }};
}

macro_rules! handle_follow_symlink {
    ($result: expr, $path:expr,  $on_err: expr) => {
        match $result {
            Ok(md) => md,
            Err(err) => {
                log::trace!(target: crate::error_handling::DISCOVERY_ERR_TARGET, "failed to follow symlink {}: {err}", $path.display());
                $on_err
            }
        }
    };
}

impl DiscoveringInputSource {
    pub fn new(recurse: bool, follow_symlink: bool, sources: Vec<Arc<LinkedPath>>, file_filters: FileFilter) -> Self {
        Self {
            recurse,
            follow_symlink,
            file_filters,
            sources,
            path_acc: Default::default(),
        }
    }
    fn handle_symlink(&mut self, entry: std::fs::DirEntry, dir_path: &Arc<LinkedPath>, sink: &mut InputSink) {
        let entry_name = entry.file_name();
        let pop_token = push_to_path(&mut self.path_acc, &entry_name);
        let metadata = handle_follow_symlink!(std::fs::metadata(&pop_token.0), pop_token.0, return);
        let entry_name = LinkedPath::new_child(&dir_path, entry_name);
        if metadata.is_file() {
            let keep_file = self.file_filters.keep_file_md(&entry_name, pop_token.0, &metadata);
            if keep_file {
                sink.put(entry_name)
            }
        } else if metadata.is_dir() && self.recurse {
            self.sources.push(Arc::new(entry_name))
        }
    }
    fn consume_entry(&mut self, entry: std::fs::DirEntry, dir_path: &Arc<LinkedPath>, sink: &mut InputSink) {
        let file_type = handle_get_file_type!(entry.file_type(), self.path_acc, entry.file_name(), return);
        if file_type.is_file() {
            let file_name = entry.file_name();
            let pop_token = push_to_path(&mut self.path_acc, &file_name);
            let file_name = LinkedPath::new_child(dir_path, file_name);
            let keep_file = self.file_filters.keep_file_dir_entry(&file_name, pop_token.0, entry);
            if keep_file {
                sink.put(file_name);
            }
        } else if file_type.is_dir() && self.recurse {
            let dir_path = LinkedPath::new_child(dir_path, entry.file_name());
            self.sources.push(Arc::new(dir_path));
        } else if file_type.is_symlink() && self.follow_symlink {
            self.handle_symlink(entry, dir_path, sink)
        }
    }
    fn consume_one(&mut self, dir: Arc<LinkedPath>, sink: &mut InputSink) {
        dir.write_full_to_buf(&mut self.path_acc);
        let current_dir = handle_access_dir!(std::fs::read_dir(&self.path_acc), self.path_acc, return);
        for entry in current_dir {
            let entry = handle_access_dir!(entry, self.path_acc, break);
            self.consume_entry(entry, &dir, sink)
        }
    }
}

impl InputSource for DiscoveringInputSource {
    fn consume_all(&mut self, sink: &mut InputSink) -> Result<(), AlreadyReportedError> {
        while let Some(source) = self.sources.pop() {
            self.consume_one(source, sink)
        }
        Ok(())
    }
}

impl StdInSource {
    pub fn new(file_filters: FileFilter) -> Self {
        Self {
            file_filters,
        }
    }
}

impl InputSource for StdInSource {
    fn consume_all(&mut self, sink: &mut InputSink) -> Result<(), AlreadyReportedError> {
        let source = std::io::stdin().lock();
        for line in source.lines() {
            let line = line.map_err(|err| {
                log::error!(target: crate::error_handling::DISCOVERY_ERR_TARGET, "failed to read files from stdin: {err}");
                AlreadyReportedError
            })?;
            if line.is_empty() {
                continue;
            }
            let path = Arc::into_inner(LinkedPath::from_path_buf(line.as_ref())).unwrap();
            if self.file_filters.keep_file(&path, line.as_ref()) {
                sink.put(path);
            }
        }
        Ok(())
    }
}