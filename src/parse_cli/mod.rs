mod parse_file_size;


use std::collections::HashSet;
use std::ffi::OsString;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use clap::{arg, Arg, ArgAction, ArgGroup, Command, value_parser, ValueHint};
use clap::builder::{OsStr, PossibleValue, PossibleValuesParser, ValueParser};
use crate::error_handling::get_all_log_targets;

use crate::file_action::{DeleteFileAction, FileConsumeAction, ReplaceWithHardLinkFileAction};
use crate::file_filters::{ExtensionFilter, FileFilter, FileMetadataFilter, FileNameFilter, MaxSizeFileFilter, MinSizeFileFilter, PathFilter};
use crate::file_set_refiner::{FileContentEquals, FileEqualsChecker};

use crate::os::{SetOrderOption, SimpleFieEqualCheckerArg, SimpleFileConsumeActionArg};
use crate::parse_cli::parse_file_size::{FileSize, FileSizeValueParser};
use crate::set_consumer::{DryRun, FileSetConsumer, InteractiveEachChoice, MachineReadableEach, MachineReadableSet, UnconditionalAction};
use crate::set_order::{CreateTimeSetOrder, ModTimeSetOrder, NameAlphabeticSetOrder, NoopSetOrder, SetOrder};
use crate::util::LinkedPath;

pub struct ExecutionPlan {
    pub dirs: Vec<Arc<LinkedPath>>,
    pub recursive_dirs: Vec<Arc<LinkedPath>>,
    pub follow_symlinks: bool,
    pub file_equals: Vec<Box<dyn FileEqualsChecker + Send>>,
    pub order_set: Vec<Box<dyn SetOrder + Send>>,
    pub action: Box<dyn FileSetConsumer>,
    pub file_filter: FileFilter,
    pub num_threads: NonZeroU32,
    pub ignore_log_set: Vec<String>,
}

const ACTION_MODE_GROUP: &str = "action_mode";
const ACTION_MODE_ACTION_GROUP: &str = "file_action_action";
const FILE_ACTION_GROUP: &str = "file_action";
const SET_LOG_TARGET_GROUP: &str = "set_log_action";
const EXT_LIST_GROUP: &str = "ext_list";

fn assemble_command_info() -> clap::Command {
    let mut command = clap::Command::new("duplis")
        .before_help("find duplicate files; does a dry-run by default, specify an action(which can be found below) to  change that")
        .before_long_help("Find duplicate files. You can not only check based on content, but also other(potentially platform dependant) stuff like permissions.\n By default this program simply outputs equal files, in order to actually do something, you need to specify an action like delete")
        .arg(arg!(dirs: <DIRS> "The directories which should be searched for duplicates(Defaults to '.')")
            .value_hint(ValueHint::DirPath)
            .value_parser(value_parser!(std::path::PathBuf))
            .action(ArgAction::Append)
            .required(false)
        )
        .arg(arg!(uncond: -u --immediate "Execute the specified action without asking")
            .action(ArgAction::SetTrue)
            .group(ACTION_MODE_GROUP)
            .group(ACTION_MODE_ACTION_GROUP)
        )
        .arg(arg!(iact: -i --interactive "Execute the specified action after confirmation on the console")
            .action(ArgAction::SetTrue)
            .group(ACTION_MODE_GROUP)
            .group(ACTION_MODE_ACTION_GROUP)
        )
        .arg(arg!(machine_readable: --wout <STRUCTURE> "Write all duplicates pairwise to stdout")
            .value_parser([
                PossibleValue::new("pairwise").help("print duplicates in format $original,$duplicate\\n"),
                PossibleValue::new("setwise").help("print entire duplicate sets, with set members separated by comma and sets separated by \\n")
            ])
            .require_equals(true)
            .num_args(0..=1)
            .action(ArgAction::Set)
            .default_missing_value(OsStr::from("pairwise"))
            .group(ACTION_MODE_GROUP)
        )
        .arg(arg!(recurse: -r --recurse "search all listed directories recursively")
            .action(ArgAction::SetTrue)
        )
        .arg(arg!(setorder: -o --orderby <ORDERINGS>)
            .action(ArgAction::Append)
            .value_delimiter(',')
            .value_parser(set_order_parser())
            .help("specify order of files; smallest on is the original; 'r' prefix means reversed")
            .long_help("Set the order in which the elements of equal file sets are ordered\nThe smallest is considered the original\nMay contain multiple orderings in decreasing importance\nSome orderings may be prefixed with r to reverse(example rmodtime)")
            .required(false)
        )
        .arg(arg!(minfsize: --minsize <SIZE> "Only consider files with >= $minsize bytes")
            .action(ArgAction::Set)
            .required(false)
            .value_parser(ValueParser::from(FileSizeValueParser))
            .ignore_case(true)
        )
        .arg(arg!(maxfsize: --maxsize <SIZE> "Only consider files with < $maxsize bytes")
            .action(ArgAction::Set)
            .required(false)
            .value_parser(ValueParser::from(FileSizeValueParser))
            .ignore_case(true)
        )
        .arg(arg!(nonzerof: -Z --nonzero "Only consider non-zero sized files").action(ArgAction::SetTrue).required(false))
        .arg(arg!(followsymlink: -s --symlink "Follow symlinks to files and directories").action(ArgAction::SetTrue).required(false))
        .arg(arg!(numthreads: -t --threads <NUM_THREADS>"Use multi-threading(optionally provide the number of threads)").action(ArgAction::Set).required(false).require_equals(true).num_args(0..=1).value_parser(value_parser!(u32)).default_missing_value(OsString::from("0")))
        .arg(arg!(logtargets: --loginfo <INFO> "update the log targets(+$TARGET turns on, ~$TARGET turns off)")
            .action(ArgAction::Append)
            .value_delimiter(',')
            .required(false)
            .value_parser(PossibleValuesParser::new(get_all_log_targets().into_iter().flat_map(|target| [format!("~{target}"), format!("+{target}")]).collect::<Vec<_>>()))
            .ignore_case(true)
            .group(SET_LOG_TARGET_GROUP)
        )
        .arg(arg!(setlogtargets: --setloginfo <INFO> "set the log targets to be logged")
            .action(ArgAction::Append)
            .required(false)
            .value_parser(PossibleValuesParser::new({
                let mut targets = get_all_log_targets();
                targets.push("~");
                targets
            }))
            .ignore_case(true)
            .group(SET_LOG_TARGET_GROUP)
        )
        .arg(arg!(extbl: --extbl <EXTENSIONS>)
            .help("files with these extensions are not processed(~ means no extension)")
            .long_help("files with these extensions are not processed(~ means no extension), extensions must be given without preceding dot(\"txt\" not \".txt\")")
            .value_delimiter(',')
            .value_parser(value_parser!(OsString))
            .action(ArgAction::Append)
            .required(false)
            .group(EXT_LIST_GROUP)
        )
        .arg(arg!(extwl: --extwl <EXTENSIONS> "ONLY files with these extensions are processed")
            .help("ONLY files with these extensions are processed(~ means no extension)")
            .long_help("ONLY files with these extensions are processed(~ means no extension), extensions must be given without preceding dot(\"txt\" not \".txt\")")
            .value_delimiter(',')
            .value_parser(value_parser!(OsString))
            .action(ArgAction::Append)
            .required(false)
            .group(EXT_LIST_GROUP)
        )
        .arg(arg!(pathbl: --pathbl <PATHS> "files with these paths as prefix will not be processed")
            .value_delimiter(',')
            .action(ArgAction::Append)
            .value_parser(value_parser!(std::path::PathBuf))
            .required(false)
        )
        .arg(arg!(pathblfiles: --pathblloc <FILES>)
            .help("points to files which serve as blacklists for path prefixes(like pathbl)")
            .long_help("points to files which serve as blacklists for path prefixes(like pathbl), the files must contain a list of \\n separated utf-8  encoded paths")
            .action(ArgAction::Append)
            .value_parser(PathListFileParser)
            .value_delimiter(',')
            .required(false)
        )
        .group(ArgGroup::new(ACTION_MODE_ACTION_GROUP).requires(FILE_ACTION_GROUP))
        .group(ArgGroup::new(FILE_ACTION_GROUP).requires(ACTION_MODE_ACTION_GROUP));
    for (name, short, long, help, _) in get_file_consume_action_args() {
        command = command.arg(Arg::new(name).short(short).long(long).help(help).action(ArgAction::SetTrue).group(FILE_ACTION_GROUP));
    }
    for (name, short, long, help, _) in get_file_equals_args() {
        command = command.arg(Arg::new(name).short(short).long(long).help(help).action(ArgAction::SetTrue))
    }
    command
}

#[derive(Clone)]
struct PathListFileParser;

impl clap::builder::TypedValueParser for PathListFileParser {
    type Value = Vec<PathBuf>;

    fn parse_ref(&self, cmd: &Command, arg: Option<&Arg>, value: &std::ffi::OsStr) -> Result<Self::Value, clap::Error> {
        use std::io::BufRead;
        let err_map = |err: std::io::Error| {
            let arg_text = arg.map_or(String::new(), |arg| {
                let literal = cmd.get_styles().get_literal();
                format!("'{}{arg}{}'", literal.render(), literal.render_reset())
            });
            let err_style = cmd.get_styles().get_error();
            clap::Error::raw(clap::error::ErrorKind::Io, format!("failed to open path file({arg_text}) {value:?}: {}{err}{}\n", err_style.render(), err_style.render_reset()))
                .with_cmd(cmd)
        };
        let file = std::fs::OpenOptions::new().read(true).open(value)
            .map_err(err_map)?;
        let paths = std::io::BufReader::new(file)
            .lines()
            .filter(|s| s.as_ref().map_or(true, |s| !s.is_empty()))
            .map(|s| s.map(std::path::PathBuf::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(err_map)?;
        Ok(paths)
    }
}

fn parse_directories(matches: &clap::ArgMatches) -> Vec<Arc<LinkedPath>> {
    matches.get_many::<std::path::PathBuf>("dirs")
        .map(|paths| paths.map(|buf| buf.as_path()).map(LinkedPath::from_path_buf).collect::<Vec<_>>())
        .unwrap_or_else(|| vec![LinkedPath::root(".")])
}

fn parse_set_order(matches: &clap::ArgMatches) -> Vec<Box<dyn SetOrder + Send>> {
    let mut order = match matches.get_many::<String>("setorder") {
        Some(options) => {
            let variants = get_set_order_options();
            options.map(|sname| variants.iter().find(|(name, _, _)| name == sname).unwrap().2.dyn_clone()).collect::<Vec<_>>()
        }
        None => {
            let default: Box<dyn SetOrder + Send> = Box::new(ModTimeSetOrder::new(false));
            vec![default]
        }
    };
    order.reverse();
    order
}

fn parse_ignore_log_targets(matches: &clap::ArgMatches) -> Vec<String> {
    if let Some(targets) = matches.get_many::<String>("setlogtargets") {
        let all_targets = get_all_log_targets();
        let targets = targets
            .map(|it| it.to_ascii_lowercase())
            .filter(|s| s != "~")
            .collect::<HashSet<String>>();
        all_targets.into_iter().filter(|s| !targets.contains(*s)).map(|s| s.to_owned()).collect::<Vec<_>>()
    } else if let Some(target_change) = matches.get_many::<String>("logtargets") {
        let mut default_ignore = HashSet::new();
        let changes = target_change
            .map(|it| (it.starts_with('+'), &it[1..]))
            .map(|(positive, target)| (target.to_ascii_lowercase(), positive));
        for (target, positive) in changes {
            if positive {
                // if we want to log the target, we need to remove it from the ignore list
                default_ignore.remove(&target);
            } else {
                default_ignore.insert(target);
            }
        }
        default_ignore.into_iter().collect::<Vec<_>>()
    } else {
        vec![]
    }
}

fn parse_path_blacklist(matches: &clap::ArgMatches) -> Option<Box<dyn FileNameFilter>> {
    let mut blacklisted = Vec::new();
    if let Some(bl) = matches.get_many::<PathBuf>("pathbl") {
        blacklisted.extend(bl);
    }
    if let Some(bls) = matches.get_many::<Vec<PathBuf>>("pathblfiles") {
        blacklisted.extend(bls.flatten())
    }
    if blacklisted.is_empty() {
        None
    } else {
        let filter = PathFilter::new(blacklisted.iter().map(|p| p.as_path()));
        Some(Box::new(filter))
    }
}

fn get_set_order_options() -> Vec<(&'static str, String, Box<dyn SetOrder>)> {
    let default_order_options: Vec<(&'static str, Box<dyn SetOrder>, &'static str)> = vec![
        ("modtime", Box::new(ModTimeSetOrder::new(false)), "Order the files from least recently to most recently modified"),
        ("rmodtime", Box::new(ModTimeSetOrder::new(true)), "Order the files from most recently to least recently modified"),
        ("createtime", Box::new(CreateTimeSetOrder::new(false)), "Order the files from oldest to newest"),
        ("rcreatetime", Box::new(CreateTimeSetOrder::new(true)), "Order the files from newest to oldest"),
        ("alphabetic", Box::new(NameAlphabeticSetOrder::new(false)), "Order the files alphabetically ascending(may behave strangely with chars that are not ascii letters or digits)"),
        ("ralphabetic", Box::new(NameAlphabeticSetOrder::new(true)), "Order the files alphabetically descending(risks and side effects of 'alphabetic' apply)"),
        ("as_is", Box::new(NoopSetOrder::new()), "Do not order the files; the order is thus non-deterministic and not reproducible"),
    ];
    let default_order_options = default_order_options.into_iter()
        .map(|(name, action, help)| (name, String::from(help), action));

    let os_options = crate::os::get_set_order_options().into_iter()
        .map(|SetOrderOption { name, help, implementation }| (name, help, implementation));

    default_order_options.chain(os_options).collect::<Vec<_>>()
}

fn get_file_consume_action_args() -> Vec<(&'static str, char, &'static str, String, Box<dyn FileConsumeAction + Send>)> {
    let mut default: Vec<(_, _, _, _, Box<dyn FileConsumeAction + Send>)> = vec![
        ("isdel", 'd', "delete", String::from("Delete duplicated files"), Box::new(DeleteFileAction::default())),
        ("rehl", 'l', "rehardlink", String::from("Replace duplicated files with a hard link"), Box::new(ReplaceWithHardLinkFileAction::default())),
    ];
    let os_specific = crate::os::get_file_consumer_simple()
        .into_iter()
        .map(|SimpleFileConsumeActionArg { name, short, long, help, action }| (name, short, long, help, action));
    default.extend(os_specific);
    default
}

fn get_file_equals_args() -> Vec<(&'static str, char, &'static str, String, Box<dyn FileEqualsChecker + Send>)> {
    let mut default: Vec<(_, _, _, _, Box<dyn FileEqualsChecker + Send>)> = vec![
        ("contenteq", 'c', "contenteq", String::from("compare files byte-by-byte"), Box::new(FileContentEquals::default()))
    ];
    let os_specific = crate::os::get_file_equals_simple()
        .into_iter()
        .map(|SimpleFieEqualCheckerArg { name, short, long, help, action }| (name, short, long, help, action));
    default.extend(os_specific);
    default
}

fn set_order_parser() -> clap::builder::ValueParser {
    let values = get_set_order_options()
        .into_iter()
        .map(|(name, help, _)| PossibleValue::new(name.clone()).help(help))
        .collect::<Vec<_>>();

    PossibleValuesParser::new(values)
        .into()
}

pub fn parse() -> Result<ExecutionPlan, ()> {
    let matches = assemble_command_info()
        .get_matches();
    //let x = matches.get_many::<usize>("oi").unwrap();

    let mut dirs = parse_directories(&matches);
    let mut rec_dirs = vec![];

    let file_filter = {
        let mut filename_filter: Vec<Box<dyn FileNameFilter>> = Vec::new();
        let mut metadata_filter: Vec<Box<dyn FileMetadataFilter>> = Vec::new();
        if let Some(filter) = matches.get_one::<FileSize>("maxfsize") {
            metadata_filter.push(Box::new(MaxSizeFileFilter::new(filter.0)))
        }
        if let Some(filter) = matches.get_one::<FileSize>("minfsize") {
            metadata_filter.push(Box::new(MinSizeFileFilter::new(filter.0.saturating_sub(1))))
        }
        if matches.get_flag("nonzerof") {
            metadata_filter.push(Box::new(MinSizeFileFilter::new(0)))
        }
        fn gather_exts<'a>(exts: impl Iterator<Item=&'a OsString>) -> (HashSet<OsString>, bool) {
            let mut exts_col = HashSet::with_capacity(exts.size_hint().0);
            let mut no_ext = false;
            let curly = OsString::from("~");
            for ext in exts {
                if ext == &curly {
                    no_ext = true;
                } else {
                    exts_col.insert(ext.clone());
                }
            }
            (exts_col, no_ext)
        }
        if let Some(exts) = matches.get_many::<OsString>("extbl") {
            let (exts, no_ext) = gather_exts(exts);
            let filter = ExtensionFilter::new(exts, no_ext, false);
            filename_filter.push(Box::new(filter));
        }
        if let Some(exts) = matches.get_many::<OsString>("extwl") {
            let (exts, no_ext) = gather_exts(exts);
            let filter = ExtensionFilter::new(exts, no_ext, true);
            filename_filter.push(Box::new(filter));
        }
        if let Some(filter) = parse_path_blacklist(&matches) {
            filename_filter.push(filter)
        }
        FileFilter(filename_filter.into_boxed_slice(), metadata_filter.into_boxed_slice())
    };

    let num_threads = match matches.get_one::<u32>("numthreads") {
        Some(0) => {
            u32::try_from(std::thread::available_parallelism().map_or(1, NonZeroUsize::get).saturating_mul(2)).unwrap_or(u32::MAX)
        }
        Some(num) => *num,
        None => 1
    };

    let recurse = matches.get_flag("recurse");
    let follow_symlinks = matches.get_flag("followsymlink");

    let set_ordering = parse_set_order(&matches);

    let file_action: Option<Box<dyn FileConsumeAction + Send>> = get_file_consume_action_args()
        .into_iter()
        .map(|(name, _, _, _, i)| (name, i))
        .find(|(name, _)| matches.get_flag(name))
        .map(|(_, i)| i);

    let file_equals = get_file_equals_args()
        .into_iter()
        .map(|(name, _, _, _, i)| (name, i))
        .filter(|(name, _)| matches.get_flag(name))
        .map(|(_, i)| i)
        .collect::<Vec<_>>();

    let file_set_consumer: Box<dyn FileSetConsumer> = if matches.get_flag("uncond") {
        Box::new(UnconditionalAction::new(file_action.expect("file action should be present because of command config")))
    } else if matches.get_flag("iact") {
        Box::new(InteractiveEachChoice::for_console(file_action.expect("file action should be present because of command config")))
    } else if let Some(kind) = matches.get_one::<String>("machine_readable") {
        match kind.as_str() {
            "pairwise" => Box::new(MachineReadableEach::for_console()),
            "setwise" => Box::new(MachineReadableSet::for_console()),
            _ => panic!("invalid maschine-reable-out config {kind}")
        }
    } else {
        Box::new(DryRun::for_console())
    };

    let (dirs, recursive_dirs) = if recurse {
        rec_dirs.append(&mut dirs);
        (dirs, rec_dirs)
    } else {
        (dirs, rec_dirs)
    };

    let ignore_log_set = parse_ignore_log_targets(&matches);

    let plan = ExecutionPlan {
        dirs,
        recursive_dirs,
        follow_symlinks,
        file_equals,
        order_set: set_ordering,
        action: file_set_consumer,
        file_filter,
        num_threads: NonZeroU32::new(num_threads).unwrap(),
        ignore_log_set,
    };
    Ok(plan)
}