use std::borrow::Cow;
use std::collections::{HashSet};
use std::ffi::OsString;
use std::io::{Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use crate::file_action::{FileConsumeAction, FileConsumeResult};
use crate::file_filters::{ExtensionFilter, FileMetadataFilter, FileNameFilter, MaxSizeFileFilter, MinSizeFileFilter, PathFilter};
use crate::HashedFile;
use crate::set_consumer::{FileSetConsumer, InteractiveEachChoice, MachineReadableEach, MachineReadableSet, UnconditionalAction};
use crate::set_order::{CreateTimeSetOrder, ModTimeSetOrder, NameAlphabeticSetOrder, NoopSetOrder, SetOrder};
use crate::util::LinkedPath;

type CreateFileRet = (std::fs::File, LinkedPath);

fn create_file(path: &impl AsRef<std::path::Path>, content: &[u8]) -> CreateFileRet {
    let path = path.as_ref();
    let mut final_path = PathBuf::new();
    final_path.push("test_files");
    final_path.push(path);
    let path = &final_path;
    let _ = std::fs::create_dir_all(final_path.parent().unwrap());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .expect("failed to create file");
    file.write(content).expect("failed to write to file");
    let path =  std::sync::Arc::into_inner(LinkedPath::from_path_buf(&path)).unwrap();
    (file, path)
}

fn gather_hashed_files(files: &[&(std::fs::File, LinkedPath)]) -> Vec<HashedFile> {
    files.into_iter().map(|(file, path)| HashedFile { file_version_timestamp: file.metadata().unwrap().modified().ok(), file_path: (*path).clone() }).collect()
}

fn permute<T: Clone>(source: &[T], indices: &[usize]) -> Vec<T> {
    indices.iter().copied().map(|i| source[i].clone()).collect()
}

struct UnreachableFileConsumer;

impl FileConsumeAction for UnreachableFileConsumer {
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult {
        panic!("should be unreachable: {path:?}, {original:?}")
    }

    fn requires_original(&self) -> bool {
        true
    }

    fn short_name(&self) -> Cow<str> {
        Cow::Borrowed("fail")
    }

    fn short_opposite(&self) -> Cow<str> {
        Cow::Borrowed("don't fail")
    }
}

#[derive(Clone)]
struct ExpectingConsumeAction(HashSet<(PathBuf, Option<PathBuf>)>);

impl FileConsumeAction for ExpectingConsumeAction {
    fn consume(&mut self, path: &Path, original: Option<&Path>) -> FileConsumeResult {
        assert!(self.0.remove(&(path.to_path_buf(), original.map(Path::to_path_buf))));
        Ok(())
    }

    fn requires_original(&self) -> bool {
        false
    }

    fn short_name(&self) -> Cow<str> {
        Cow::Borrowed("check for")
    }

    fn short_opposite(&self) -> Cow<str> {
        Cow::Borrowed("ignore")
    }
}

impl Drop for ExpectingConsumeAction {
    fn drop(&mut self) {
        assert!(self.0.is_empty(), "this is not empty:{:?}", self.0);
    }
}

pub struct CommonPrefix(&'static str, u64, Vec<PathBuf>);
impl CommonPrefix {
    pub fn new(prefix: &'static str) -> Self {
        Self(prefix, 0, vec![])
    }
    pub fn create_file(&mut self, path: &str, content: &[u8]) -> CreateFileRet {
        let path = PathBuf::from(&format!("{}{path}", self.0));
        let result = create_file(&path, content);
        self.2.push(path);
        return result
    }
    pub fn create_file_auto(&mut self, content: &[u8]) -> CreateFileRet {
        self.1 += 1;
        self.create_file(&format!("{}", self.1), content)
    }
    pub fn make_file_auto(&mut self) -> CreateFileRet {
        self.create_file_auto(&[])
    }
}

impl Drop for CommonPrefix {
    fn drop(&mut self) {
        for file in &self.2 {
            let _ = std::fs::remove_file(file);
        }
    }
}

fn test_named_filter(files: &Vec<(LinkedPath, PathBuf)>, expected: &[usize], mut filterer: impl FileNameFilter) {
    let filtered = files.iter()
        .filter(|(path, buf)| filterer.filter_file_name(path, buf).unwrap())
        .collect::<Vec<_>>();
    let expected = permute(&files, expected);
    let expected = expected.iter().map(|(_, buf)| buf).collect::<Vec<_>>();
    let filtered = filtered.into_iter().map(|(_, buf)| buf).collect::<Vec<_>>();
    assert_eq!(filtered, expected);
}

#[test]
fn test_ordering() {
    let mut prefix = CommonPrefix::new("ordering_");
    let file1 = prefix.create_file("first", &[]);
    sleep(Duration::from_millis(50));
    let mut file2 = prefix.create_file("2", &[]);
    sleep(Duration::from_millis(50));
    let mut file3 = prefix.create_file("z", &[]);
    sleep(Duration::from_millis(50));
    let file4 = prefix.create_file("ABC", &[4, 5]);
    sleep(Duration::from_millis(50));
    file2.0.write(b"asodadas").unwrap();
    sleep(Duration::from_millis(50));
    file3.0.write(b"nadas").unwrap();
    let files = gather_hashed_files([&file1,  &file2, &file3, &file4].as_slice());

    fn test_ordering(files: &Vec<HashedFile>, expected: &[usize], mut orderer: impl SetOrder) {
        let mut files1 = files.clone();
        orderer.order(&mut files1).unwrap();
        let expected = permute(&files, expected);
        assert_eq!(files1, expected)
    }

    test_ordering(&files, &[0, 3, 1, 2], ModTimeSetOrder::new(false));
    test_ordering(&files, &[2, 1, 3, 0], ModTimeSetOrder::new(true));
    test_ordering(&permute(&files, &[2, 0, 3, 1]), &[1, 3, 0, 2], CreateTimeSetOrder::new(false));
    test_ordering(&permute(&files, &[2, 0, 3, 1]), &[2, 0, 3, 1], CreateTimeSetOrder::new(true));
    test_ordering(&files, &[1, 3, 0, 2], NameAlphabeticSetOrder::new(false));
    test_ordering(&files, &[2, 0, 3, 1], NameAlphabeticSetOrder::new(true));
    test_ordering(&files, &[0, 1, 2, 3], NoopSetOrder::new());

    files.into_iter().for_each(|HashedFile { file_path, .. }| std::fs::remove_file(file_path.to_push_buf()).unwrap())
}

#[test]
fn test_file_filter() {
    let mut prefix = CommonPrefix::new("file_filter_");
    let file0 = prefix.create_file_auto(b"");
    let file1 = prefix.create_file_auto(b"b");
    let file2 = prefix.create_file_auto(b"bb");
    let file3 = prefix.create_file_auto(b"bbb");
    let file4 = prefix.create_file_auto(b"bbbb");
    let files = gather_hashed_files(&[&file0, &file1, &file2 , &file3, &file4])
        .into_iter()
        .map(|HashedFile { file_path, .. }| (file_path.to_push_buf(), file_path))
        .map(|(path, lpath)| (lpath, path.metadata().unwrap(), path))
        .collect::<Vec<_>>();

    fn test_filter(files: &Vec<(LinkedPath, std::fs::Metadata,  PathBuf)>, expected: &[usize], mut filterer: impl FileMetadataFilter) {
        let filtered = files.iter().filter(|(path, md, buf)| filterer.filter_file_metadata(path, buf, md).unwrap())
            .collect::<Vec<_>>();
        let expected = permute(&files, expected);
        let expected = expected.iter().map(|(_, _, buf)| buf).collect::<Vec<_>>();
        let filtered = filtered.into_iter().map(|(_, _, buf)| buf).collect::<Vec<_>>();
        assert_eq!(filtered, expected);
    }
    test_filter(&files, &[1, 2, 3, 4], MinSizeFileFilter::new(0));
    test_filter(&files, &[2, 3, 4,], MinSizeFileFilter::new(1));
    test_filter(&files, &[], MinSizeFileFilter::new(4));
    test_filter(&files, &[0, 1, 2, 3], MaxSizeFileFilter::new(4));
    test_filter(&files, &[0, 1, 2], MaxSizeFileFilter::new(3));
    test_filter(&files, &[0], MaxSizeFileFilter::new(1));
    test_filter(&files, &[], MaxSizeFileFilter::new(0));

    files.into_iter().for_each(|(_, _, file)| std::fs::remove_file(file).unwrap())
}

#[test]
fn test_filter_extension() {
    let mut prefix = CommonPrefix::new("test_filter_extension_");
    let file0 = prefix.create_file("ext.ea", &[]);
    let file1 = prefix.create_file("ext.eb", &[]);
    let file2 = prefix.create_file("ext.ec", &[]);
    let file3 = prefix.create_file("ext", &[]);

    let files = [&file0, &file1, &file2, &file3].into_iter()
        .map(|f| (f.1.clone(), f.1.to_push_buf()))
        .collect::<Vec<_>>();

    let filterer = ExtensionFilter::new(HashSet::from(["ea", "ec"].map(OsString::from)), false, false);

    test_named_filter(&files, &[1, 3], filterer);

    let filterer= ExtensionFilter::new(HashSet::from(["ea", "ec"].map(OsString::from)), true, false);
    test_named_filter(&files, &[1], filterer);

    let filterer= ExtensionFilter::new(HashSet::from(["ea", "ec"].map(OsString::from)), false, true);
    test_named_filter(&files, &[0, 2], filterer);


    let filterer= ExtensionFilter::new(HashSet::from(["ea", "ec"].map(OsString::from)), true, true);
    test_named_filter(&files, &[0, 2, 3], filterer);
}

#[test]
fn test_filter_path() {
    let mut prefix = CommonPrefix::new("test_filter_prefix");

    let file0 = prefix.create_file("/sdir1/ssdir1/file0", &[]);
    let file1 = prefix.create_file("/sdir1/file1", &[]);
    let file2 = prefix.create_file("/sdir1/ssdir2/file2", &[]);
    let file3 = prefix.create_file("/sdir2/ssdir1/file3", &[]);
    let file4 = prefix.create_file("/sdir2/file4", &[]);
    let file5 = prefix.create_file("file5", &[]);

    let files = [&file0, &file1, &file2, &file3, &file4, &file5].into_iter()
        .map(|f| (f.1.clone(),  f.1.to_push_buf()))
        .collect::<Vec<_>>();

    let filterer = PathFilter::new([("test_files")].into_iter().map(<str as AsRef<Path>>::as_ref));

    test_named_filter(&files, &[], filterer);

    let filterer = PathFilter::new(["test_files/test_filter_prefix/sdir1"].into_iter().map(<str as AsRef<Path>>::as_ref));

    test_named_filter(&files, &[3, 4, 5], filterer);

    let filterer = PathFilter::new(["test_files/test_filter_prefix/sdir1/ssdir1", "test_files/test_filter_prefix/sdir1"].into_iter().map(<str as AsRef<Path>>::as_ref));

    test_named_filter(&files, &[3, 4, 5], filterer);

    let filterer = PathFilter::new(["test_files/test_filter_prefix/sdir1/ssdir1"].into_iter().map(<str as AsRef<Path>>::as_ref));
    test_named_filter(&files, &[1, 2, 3, 4, 5], filterer);

    let filterer = PathFilter::new(["test_files/test_filter_prefix/sdir1/ssdir1", "test_files/test_filter_prefix/sdir2/ssdir1"].into_iter().map(<str as AsRef<Path>>::as_ref));
    test_named_filter(&files, &[1, 2, 4, 5], filterer);

    let filterer = PathFilter::new(["test_files/test_filter_prefix/sdir1/file1"].into_iter().map(<str as AsRef<Path>>::as_ref));
    test_named_filter(&files, &[0, 2, 3, 4, 5], filterer)
}

fn test_deleted_original(prefix: &mut CommonPrefix, mut consumer: impl FileSetConsumer) {
    let file1 = prefix.make_file_auto();
    let file2 = prefix.make_file_auto();
    let path_1= file1.1.to_push_buf();
    std::fs::remove_file(&path_1).unwrap();
    let files = gather_hashed_files(&[&file1, &file2]);
    consumer.consume_set(files).unwrap();
    let files = gather_hashed_files(&[&file2, &file1]);
    consumer.consume_set(files).unwrap();
}

#[test]
fn test_unconditional_set_consumer() {
    let mut prefix = CommonPrefix::new("uncond_set_consumer");
    test_deleted_original(&mut prefix, UnconditionalAction::new(Box::new(UnreachableFileConsumer)));
}

#[test]
fn test_machine_readable_each() {
    let mut prefix = CommonPrefix::new("m_read_each_");
    let empty_write: &mut [u8] = [].as_mut_slice();

    test_deleted_original(&mut prefix, MachineReadableEach::new(empty_write));

    let file1 = prefix.make_file_auto();
    let file2 = prefix.make_file_auto();
    let file3 = prefix.make_file_auto();
    let filec = prefix.create_file(",file", &[]);

    let file1p = file1.1.to_push_buf().canonicalize().unwrap();
    let file2p = file2.1.to_push_buf().canonicalize().unwrap();
    let file3p = file3.1.to_push_buf().canonicalize().unwrap();

    let mut target = Vec::new();

    let mut mreadable = MachineReadableEach::new(&mut target);

    let files = gather_hashed_files(&[&file1, &filec, &file2, &file3]);

    mreadable.consume_set(files).unwrap();

    let result = String::from_utf8(target.clone()).unwrap();
    let expected = format!("{},{}\n{},{}", file1p.display(), file2p.display(), file1p.display(), file3p.display());
    assert_eq!(result, expected);

    target.clear();
    let mut mreadable = MachineReadableEach::new(&mut target);

    mreadable.consume_set(gather_hashed_files(&[&filec, &file1, &file2, &file3])).unwrap();

    let result = String::from_utf8(target).unwrap();

    assert_eq!(result, expected);
    let mut empty_buf: [u8; 0] = [];
    let mut mreadable  = MachineReadableEach::new(empty_buf.as_mut_slice());

    mreadable.consume_set(gather_hashed_files(&[&file1, &file2])).unwrap_err();
}

#[test]
fn test_machine_readable_set() {
    let mut prefix = CommonPrefix::new("m_read_set_");
    let mut target: Vec<u8> = Vec::new();

    test_deleted_original(&mut prefix, MachineReadableSet::new(&mut target));
    assert!(target.is_empty());

    let file1 = prefix.make_file_auto();
    let file2 = prefix.make_file_auto();
    let file3 = prefix.make_file_auto();
    let filec = prefix.create_file(",file", &[]);

    let file1p = file1.1.to_push_buf().canonicalize().unwrap();
    let file2p = file2.1.to_push_buf().canonicalize().unwrap();
    let file3p = file3.1.to_push_buf().canonicalize().unwrap();

    let mut mreadable = MachineReadableSet::new(&mut target);

    let files = gather_hashed_files(&[&file1, &filec, &file2, &file3]);

    mreadable.consume_set(files).unwrap();

    let result = String::from_utf8(target.clone()).unwrap();
    let expected = format!("{},{},{}", file1p.display(), file2p.display(), file3p.display());
    assert_eq!(result, expected);

    target.clear();
    let mut mreadable = MachineReadableSet::new(&mut target);

    mreadable.consume_set(gather_hashed_files(&[&filec, &file1, &file2, &file3])).unwrap();

    let result = String::from_utf8(target).unwrap();

    assert_eq!(result, expected);
    let mut empty_buf: [u8; 0] = [];
    let mut mreadable  = MachineReadableSet::new(empty_buf.as_mut_slice());

    mreadable.consume_set(gather_hashed_files(&[&file1, &file2])).unwrap_err();
}

#[test]
fn test_interactive_set_action() {
    let mut prefix = CommonPrefix::new("interactive_set_action");

    let mut empty_write_buf: [u8; 0] = [];
    let empty_read_buf: [u8; 0] = [];

    test_deleted_original(&mut prefix, InteractiveEachChoice::new(empty_read_buf.as_slice(), empty_write_buf.as_mut_slice(), Box::new(UnreachableFileConsumer)));

    let file1 = prefix.make_file_auto();
    let file2 = prefix.make_file_auto();
    let file3 = prefix.make_file_auto();

    let file1p = file1.1.to_push_buf();
    let file2p = file2.1.to_push_buf();

    let expected = || ExpectingConsumeAction(HashSet::from([(file2p.clone(), Some(file1p.clone()))]));
    let files = gather_hashed_files(&[&file1, &file2, &file3]);

    let mut write_sink = Vec::new();
    let read_source = b"y\nn".as_ref();

    let mut writer = InteractiveEachChoice::new(read_source, &mut write_sink, Box::new(expected()));
    writer.consume_set(files).unwrap();

    let files = gather_hashed_files(&[&file1, &file3, &file2]);

    let mut write_sink = Vec::new();
    let read_source = b"no\nyes".as_slice();

    let mut writer = InteractiveEachChoice::new(read_source, &mut write_sink, Box::new(expected()));
    writer.consume_set(files).unwrap();
}