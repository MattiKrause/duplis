mod parse_file_size;
mod parse_number;

pub use parse_number::UNumberParser;

use crate::error_handling::get_all_log_targets;
use clap::builder::{OsStr, PossibleValue, PossibleValuesParser, TypedValueParser, ValueParser};
use clap::{arg, value_parser, ArgAction, ArgGroup, ValueHint};
use std::collections::HashSet;
use std::ffi::OsString;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::file_action::{DeleteFileAction, FileConsumeAction, ReplaceWithHardLinkFileAction};
use crate::file_filters::{
    ExtensionFilter, FileFilter, FileMetadataFilter, FileNameFilter, MaxSizeFileFilter,
    MinSizeFileFilter, PathFilter,
};
use crate::file_set_refiner::{FileContentEquals, FileEqualsChecker};
use crate::input_source::{DiscoveringInputSource, InputSource, StdInSource};

use crate::os::{
    complex_cmd_config, complex_parse_file_metadata_filters, FileNameFilterArg, SetOrderOption,
    SimpleFileConsumeActionArg, SimpleFileEqualCheckerArg,
};
use crate::parse_cli::parse_file_size::{FileSize, FileSizeValueParser};
use crate::set_consumer::{
    DryRun, FileSetConsumer, InteractiveEachChoice, MachineReadableEach, MachineReadableSet,
    UnconditionalAction,
};
use crate::set_order::{
    CreateTimeSetOrder, ModTimeSetOrder, NameAlphabeticSetOrder, NoopSetOrder, SetOrder,
};
use crate::util::LinkedPath;

pub struct ExecutionPlan {
    pub file_equals: Vec<Box<dyn FileEqualsChecker + Send>>,
    pub order_set: Vec<Box<dyn SetOrder + Send>>,
    pub action: Box<dyn FileSetConsumer>,
    pub num_threads: NonZeroU32,
    pub ignore_log_set: Vec<String>,
    pub input_sources: Vec<Box<dyn InputSource>>,
    pub dedup_files: bool,
}

static ACTION_MODE_GROUP: &str = "action_mode";
static ACTION_MODE_ACTION_GROUP: &str = "file_action_action";
static FILE_ACTION_GROUP: &str = "file_action";
static SET_LOG_TARGET_GROUP: &str = "set_log_action";
static EXT_LIST_GROUP: &str = "ext_list";
static INPUT_SOURCE_GROUP: &str = "input_source";
static USES_STDIN_GROUP: &str = "uses_stdin";
static DISCOVERING_SOURCE_GROUP: &str = "discovering_source";
static DISCOVERY_CONFIG_GROUP: &str = "discovery_config_source";

#[allow(clippy::too_many_lines)]
fn assemble_command_info() -> clap::Command {
    let mut command = clap::Command::new("duplis")
        .before_help("find duplicate files; does a dry-run by default, specify an action(which can be found below) to  change that")
        .before_long_help("Find duplicate files. You can not only check based on content, but also other(potentially platform dependant) stuff like permissions.\n By default this program simply outputs equal files, in order to actually do something, you need to specify an action like delete")
        .arg(arg!(dirs: <DIRS> "The directories which should be searched for duplicates")
            .value_hint(ValueHint::DirPath)
            .value_parser(CanonicalPathValueParser)
            .action(ArgAction::Append)
            .required(false)
            .group(INPUT_SOURCE_GROUP)
            .group(DISCOVERING_SOURCE_GROUP)
        )
        .arg(arg!(recurse: -r --recurse "search all listed directories recursively(requires dirs to be given via cli)")
            .action(ArgAction::SetTrue)
            .group(DISCOVERY_CONFIG_GROUP)
        )
        .arg(arg!(followsymlink: -s --symlink "follow symlinks to files and directories during discovery(requires dirs to be given  via cli)")
            .action(ArgAction::SetTrue)
            .required(false)
            .group(DISCOVERY_CONFIG_GROUP)
        )
        .arg(arg!(discoverstdin: --readin "reads the files which should be tested for duplication from stdin")
            .action(ArgAction::SetTrue)
            .group(USES_STDIN_GROUP)
            .group(INPUT_SOURCE_GROUP)
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
            .group(USES_STDIN_GROUP)
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
        );
    command = apply_all_args(command, get_file_consume_action_args().into_iter());

    command = command
        .arg(arg!(numthreads: -t --threads <NUM_THREADS> "Use multi-threading(optionally provide the number of threads)")
            .action(ArgAction::Set)
            .required(false)
            .require_equals(true)
            .num_args(0..=1)
            .value_parser(value_parser!(u32))
            .default_missing_value(OsString::from("0"))
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
        .arg(arg!(nonzerof: -Z --nonzero "Only consider non-zero sized files")
            .action(ArgAction::SetTrue)
            .required(false)
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
        .arg(arg!(pathbl: --pathbl <PATHS> "files with these paths as prefix will not be processed(symlinks are resolved)")
            .value_hint(ValueHint::AnyPath)
            .value_delimiter(',')
            .action(ArgAction::Append)
            .value_parser(CanonicalPathValueParser)
            .required(false)
        )
        .arg(arg!(pathblfiles: --pathblloc <FILES>)
            .help("points to files which serve as blacklists for path prefixes(like pathbl)")
            .long_help("points to files which serve as blacklists for path prefixes(like pathbl), the files must contain a list of \\n separated utf-8  encoded paths")
            .value_hint(ValueHint::FilePath)
            .action(ArgAction::Append)
            .value_parser(PathListFileParser)
            .value_delimiter(',')
            .required(false)
        );
    command = apply_all_args(command, get_file_name_filters().into_iter());
    command = apply_all_args(command, get_file_equals_args().into_iter());
    command = command
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
        .group(ArgGroup::new(INPUT_SOURCE_GROUP).required(true).multiple(true))
        .group(ArgGroup::new(ACTION_MODE_ACTION_GROUP).requires(FILE_ACTION_GROUP))
        .group(ArgGroup::new(FILE_ACTION_GROUP).requires(ACTION_MODE_ACTION_GROUP))
        .group(ArgGroup::new(DISCOVERY_CONFIG_GROUP).requires(DISCOVERING_SOURCE_GROUP).multiple(true));

    complex_cmd_config(command)
}

struct SimpleArgDeclaration<T> {
    name: &'static str,
    short: Option<char>,
    long: &'static str,
    help: String,
    is_default: bool,
    action: T,
}

fn apply_all_args<T>(
    mut command: clap::Command,
    args: impl Iterator<Item = SimpleArgDeclaration<T>>,
) -> clap::Command {
    for SimpleArgDeclaration {
        name,
        short,
        long,
        help,
        is_default,
        action: _,
    } in args
    {
        command = command.arg(
            clap::Arg::new(name)
                .short(short)
                .long(long)
                .help(help)
                .action(if is_default {
                    ArgAction::SetFalse
                } else {
                    ArgAction::SetTrue
                }),
        );
    }
    command
}

#[derive(Clone)]
struct PathListFileParser;

impl clap::builder::TypedValueParser for PathListFileParser {
    type Value = Vec<PathBuf>;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        use std::io::BufRead;
        let err_map = |err: std::io::Error| {
            let arg_text = arg.map_or(String::new(), |arg| {
                let literal = cmd.get_styles().get_literal();
                format!(
                    "(for '{}{arg}{}')",
                    literal.render(),
                    literal.render_reset()
                )
            });
            let err_style = cmd.get_styles().get_error();
            clap::Error::raw(
                clap::error::ErrorKind::Io,
                format!(
                    "failed to open path file({arg_text}) {value:?}: {}{err}{}\n",
                    err_style.render(),
                    err_style.render_reset()
                ),
            )
            .with_cmd(cmd)
        };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(value)
            .map_err(err_map)?;
        let mut paths = std::io::BufReader::new(file)
            .lines()
            .filter(|s| s.as_ref().map_or(true, |s| !s.is_empty()))
            .map(|s| s.map(std::path::PathBuf::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(err_map)?;

        for path in &mut paths {
            *path = path.canonicalize().map_err(|err| {
                let arg_text = arg.map_or(String::new(), |arg| {
                    let literal = cmd.get_styles().get_literal();
                    format!("(for '{}{arg}{}')", literal.render(), literal.render_reset())
                });
                let err_style = cmd.get_styles().get_error();
                clap::Error::raw(clap::error::ErrorKind::Io, format!("failed to canonicalize path {} from file {:?}({arg_text}) {value:?}: {}{err}{}\n", path.display(), value, err_style.render(), err_style.render_reset()))
                    .with_cmd(cmd)
            })?;
        }
        Ok(paths)
    }
}

#[derive(Clone)]
pub struct CanonicalPathValueParser;

impl TypedValueParser for CanonicalPathValueParser {
    type Value = std::path::PathBuf;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value: &std::path::Path = value.as_ref();
        value.canonicalize().map_err(|err| {
            let arg_text = arg.map_or(String::new(), |arg| {
                let literal = cmd.get_styles().get_literal();
                format!(
                    "(for '{}{arg}{}')",
                    literal.render(),
                    literal.render_reset()
                )
            });
            let err_style = cmd.get_styles().get_error();
            clap::Error::raw(
                clap::error::ErrorKind::Io,
                format!(
                    "failed to canonicalize path {} ({arg_text}) {value:?}: {}{err}{}\n",
                    value.display(),
                    err_style.render(),
                    err_style.render_reset()
                ),
            )
            .with_cmd(cmd)
        })
    }
}

fn parse_directories(matches: &clap::ArgMatches) -> Vec<Arc<LinkedPath>> {
    matches
        .get_many::<std::path::PathBuf>("dirs")
        .map(|paths| {
            paths
                .map(PathBuf::as_path)
                .map(LinkedPath::from_path_buf)
                .collect::<Vec<_>>()
        })
        .unwrap_or(Vec::new())
}

fn parse_set_order(matches: &clap::ArgMatches) -> Vec<Box<dyn SetOrder + Send>> {
    let mut order = matches
        .get_many::<String>("setorder")
        .map_or(Vec::new(), |options| {
            let variants = get_set_order_options();
            options
                .map(|sname| {
                    variants
                        .iter()
                        .find(|(name, _, _)| name == sname)
                        .unwrap()
                        .2
                        .dyn_clone()
                })
                .collect::<Vec<_>>()
        });
    if order.is_empty() {
        order.push(Box::new(ModTimeSetOrder::new(false)));
    }
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
        all_targets
            .into_iter()
            .filter(|s| !targets.contains(*s))
            .map(std::borrow::ToOwned::to_owned)
            .collect::<Vec<_>>()
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

fn parse_path_blacklist(matches: &clap::ArgMatches) -> Option<Box<dyn FileNameFilter + Send>> {
    let mut blacklisted = Vec::new();
    if let Some(bl) = matches.get_many::<PathBuf>("pathbl") {
        blacklisted.extend(bl);
    }
    if let Some(bls) = matches.get_many::<Vec<PathBuf>>("pathblfiles") {
        blacklisted.extend(bls.flatten());
    }
    if blacklisted.is_empty() {
        None
    } else {
        let filter = PathFilter::new(blacklisted.iter().map(|p| p.as_path()));
        Some(Box::new(filter))
    }
}

fn parse_file_filter(matches: &clap::ArgMatches) -> FileFilter {
    fn gather_exts<'a>(exts: impl Iterator<Item = &'a OsString>) -> (HashSet<OsString>, bool) {
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

    let mut filename_filter: Vec<Box<dyn FileNameFilter + Send>> = Vec::new();
    let mut metadata_filter: Vec<Box<dyn FileMetadataFilter + Send>> = Vec::new();
    if let Some(filter) = matches.get_one::<FileSize>("maxfsize") {
        metadata_filter.push(Box::new(MaxSizeFileFilter::new(filter.0)));
    }
    if let Some(filter) = matches.get_one::<FileSize>("minfsize") {
        metadata_filter.push(Box::new(MinSizeFileFilter::new(filter.0.saturating_sub(1))));
    }

    let additional = get_file_name_filters()
        .into_iter()
        .filter(|arg| matches.get_flag(arg.name))
        .map(|arg| arg.action);

    metadata_filter.append(&mut complex_parse_file_metadata_filters(matches));

    filename_filter.extend(additional);
    if matches.get_flag("nonzerof") {
        metadata_filter.push(Box::new(MinSizeFileFilter::new(0)));
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
    if let Some(filter) = parse_path_blacklist(matches) {
        filename_filter.push(filter);
    }
    FileFilter(
        filename_filter.into_boxed_slice(),
        metadata_filter.into_boxed_slice(),
    )
}

fn parse_input_source(matches: &clap::ArgMatches) -> Vec<Box<dyn InputSource>> {
    let mut input_source: Vec<Box<dyn InputSource>> = Vec::new();

    let recurse = matches.get_flag("recurse");
    let follow_symlinks = matches.get_flag("followsymlink");
    let read_from_stdin = matches.get_flag("discoverstdin");

    let dirs = parse_directories(matches);

    let file_filter = parse_file_filter(matches);

    if !dirs.is_empty() {
        let source =
            DiscoveringInputSource::new(recurse, follow_symlinks, dirs, file_filter.clone());
        input_source.push(Box::new(source));
    }

    if read_from_stdin {
        input_source.push(Box::new(StdInSource::new(file_filter)));
    }

    input_source
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
    let default_order_options = default_order_options
        .into_iter()
        .map(|(name, action, help)| (name, String::from(help), action));

    let os_options = crate::os::get_set_order_options().into_iter().map(
        |SetOrderOption {
             name,
             help,
             implementation,
         }| (name, help, implementation),
    );

    default_order_options.chain(os_options).collect::<Vec<_>>()
}

fn get_file_consume_action_args() -> Vec<SimpleArgDeclaration<Box<dyn FileConsumeAction + Send>>> {
    let default: Vec<(_, _, _, _, _, Box<dyn FileConsumeAction + Send>)> = vec![
        (
            "isdel",
            Some('d'),
            "delete",
            String::from("Delete duplicated files"),
            false,
            Box::<DeleteFileAction>::default(),
        ),
        (
            "rehl",
            Some('l'),
            "rehardlink",
            String::from("Replace duplicated files with a hard link"),
            false,
            Box::<ReplaceWithHardLinkFileAction>::default(),
        ),
    ];
    let os_specific = crate::os::get_file_consumer_simple().into_iter().map(
        |SimpleFileConsumeActionArg {
             name,
             short,
             long,
             help,
             default,
             action,
         }| (name, short, long, help, default, action),
    );
    default
        .into_iter()
        .chain(os_specific)
        .map(
            |(name, short, long, help, is_default, action)| SimpleArgDeclaration {
                name,
                short,
                long,
                help,
                is_default,
                action,
            },
        )
        .collect::<Vec<_>>()
}

fn get_file_equals_args() -> Vec<SimpleArgDeclaration<Box<dyn FileEqualsChecker + Send>>> {
    let default: Vec<(_, _, _, _, _, Box<dyn FileEqualsChecker + Send>)> = vec![(
        "contenteq",
        Some('c'),
        "nocontenteq",
        String::from("do not compare files byte-by-byte(only by hash)"),
        true,
        Box::new(FileContentEquals::new()),
    )];
    let os_specific = crate::os::get_file_equals_simple().into_iter().map(
        |SimpleFileEqualCheckerArg {
             name,
             short,
             long,
             help,
             default,
             action,
         }| (name, short, long, help, default, action),
    );
    default
        .into_iter()
        .chain(os_specific)
        .map(
            |(name, short, long, help, is_default, action)| SimpleArgDeclaration {
                name,
                short,
                long,
                help,
                is_default,
                action,
            },
        )
        .collect::<Vec<_>>()
}

fn get_file_name_filters() -> Vec<SimpleArgDeclaration<Box<dyn FileNameFilter + Send>>> {
    let mut default = Vec::new();
    let os_specific = crate::os::get_file_name_filters().into_iter().map(
        |FileNameFilterArg {
             name,
             short,
             long,
             help,
             default,
             action,
         }| SimpleArgDeclaration {
            name,
            short,
            long,
            help,
            is_default: default,
            action,
        },
    );
    default.extend(os_specific);
    default
}

fn set_order_parser() -> clap::builder::ValueParser {
    let values = get_set_order_options()
        .into_iter()
        .map(|(name, help, _)| PossibleValue::new(name).help(help))
        .collect::<Vec<_>>();

    PossibleValuesParser::new(values).into()
}

pub fn parse() -> ExecutionPlan {
    let matches = assemble_command_info().get_matches();
    //let x = matches.get_many::<usize>("oi").unwrap();

    let num_threads = match matches.get_one::<u32>("numthreads") {
        Some(0) => u32::try_from(
            std::thread::available_parallelism()
                .map_or(1, NonZeroUsize::get)
                .saturating_mul(2),
        )
        .unwrap_or(1),
        Some(num) => *num,
        None => 1,
    };

    let set_ordering = parse_set_order(&matches);

    let file_action: Option<Box<dyn FileConsumeAction + Send>> = get_file_consume_action_args()
        .into_iter()
        .find(|arg| matches.get_flag(arg.name))
        .map(|arg| arg.action);

    let file_equals = get_file_equals_args()
        .into_iter()
        .filter(|arg| matches.get_flag(arg.name))
        .map(|arg| arg.action)
        .collect::<Vec<_>>();

    let file_set_consumer: Box<dyn FileSetConsumer> = if matches.get_flag("uncond") {
        Box::new(UnconditionalAction::new(file_action.expect(
            "file action should be present because of command config",
        )))
    } else if matches.get_flag("iact") {
        Box::new(InteractiveEachChoice::for_console(file_action.expect(
            "file action should be present because of command config",
        )))
    } else if let Some(kind) = matches.get_one::<String>("machine_readable") {
        match kind.as_str() {
            "pairwise" => Box::new(MachineReadableEach::for_console()),
            "setwise" => Box::new(MachineReadableSet::for_console()),
            _ => panic!("invalid maschine-reable-out config {kind}"),
        }
    } else {
        Box::new(DryRun::for_console())
    };

    let input_sources = parse_input_source(&matches);

    let ignore_log_set = parse_ignore_log_targets(&matches);

    let dedup_files = matches.get_flag("followsymlink");

    ExecutionPlan {
        file_equals,
        order_set: set_ordering,
        action: file_set_consumer,
        num_threads: NonZeroU32::new(num_threads).unwrap(),
        ignore_log_set,
        input_sources,
        dedup_files,
    }
}
