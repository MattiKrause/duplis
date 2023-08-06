use std::fs::Metadata;
use std::path::Path;
use clap::{arg, ArgAction, value_parser};
use crate::file_filters::FileMetadataFilter;
use crate::parse_cli::UNumberParser;
use crate::util::LinkedPath;

pub fn complex_cmd_config(command: clap::Command) -> clap::Command {
    command
        .arg(arg!(file_attr_filter: --fattrfilter <MASK> "only process files who do not match mask")
            .action(ArgAction::Append)
            .value_parser(UNumberParser::u32())
            .value_delimiter(',')
        )
        .arg(arg!(no_hidden: --fattrfilter "only process non hidden files")
            .action(ArgAction::SetTrue)
        )
}

fn parse_file_attr_filter(matches: &clap::ArgMatches) -> Box<dyn FileMetadataFilter + Send> {
    let mut filter = 0x00000004;

    if let Some(masks) = matches.get_many::<u32>("file_attr_filter") {
        filter |= masks.fold(0, |a, b| (a | b));
    }

    if matches.get_flag("no_hidden") {
        filter |= 0x00000002;
    }

    Box::new(FileAttributeFilter { mask: filter })
}

pub fn complex_parse_file_metadata_filter(matches: &clap::ArgMatches) -> Vec<Box<dyn FileMetadataFilter + Send>> {
    vec![parse_file_attr_filter(matches)]
}

#[derive(Clone)]
struct FileAttributeFilter {
    mask: u32
}

impl FileMetadataFilter for FileAttributeFilter {
    fn filter_file_metadata(&mut self, _name: &LinkedPath, _name_path: &Path, metadata: &Metadata) -> Result<bool, ()> {
        use std::os::windows::fs::MetadataExt;

        let attrs = metadata.file_attributes();
        Ok((attrs & self.mask) == 0)
    }
}