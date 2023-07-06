use std::path::PathBuf;
use crate::HashedFile;

pub trait EqualFileSetConsumer {
    // first element of set is the 'original'
    fn consume_set(&mut self, set: Vec<HashedFile>);
}

#[derive(Default)]
pub struct DryRun(PathBuf);

impl DryRun {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EqualFileSetConsumer for DryRun {
    fn consume_set(&mut self, set: Vec<HashedFile>) {
        self.0.clear();
        set[0].file_path.push_full_to_buf(&mut self.0);
        print!("keeping {}, deleting ", self.0.display());
        let mut write_sep = false;
        for file in &set[1..] {
            if write_sep {
                print!(", ");
            }
            write_sep = true;
            self.0.clear();
            file.file_path.push_full_to_buf(&mut self.0);
            print!("{}", self.0.display());
        }
        println!();
        self.0.clear();
    }
}