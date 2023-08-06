use clap::builder::{PossibleValuesParser, StringValueParser, TypedValueParser};
use clap::error::{
    ContextKind as ClapContextKind, ContextValue as ClapContextValue, ErrorKind as ClapErrorKind,
};
use clap::Arg;
use std::ffi::OsStr;

#[derive(Clone)]
pub(crate) struct FileSize(pub u64);

/// Parse file size in binary/octal/hexadecimal with '_' separators and scale KB to EB and KiB to EiB
#[derive(Clone)]
pub(crate) struct FileSizeValueParser;

impl TypedValueParser for FileSizeValueParser {
    type Value = FileSize;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = StringValueParser::new().parse_ref(cmd, arg, value)?;

        let (off, ranges, radix) = if value.starts_with("0b") | value.starts_with("0B") {
            (2, [(0, b'0', b'1' + 1)].as_slice(), 2)
        } else if value.starts_with("0x") | value.starts_with("0X") {
            (
                2,
                [(0, b'0', b'9' + 1), (10, b'A', b'F' + 1)].as_slice(),
                16,
            )
        } else if value.starts_with("0o") | value.starts_with("0O") {
            (2, [(0, b'0', b'7' + 1)].as_slice(), 8)
        } else {
            (0, [(0, b'0', b'9' + 1)].as_slice(), 10)
        };

        let (fs_literal, rem) = parse_number_prefix(&value[off..], ranges, radix)
            .map_err(|err| format_int_err(err, cmd, arg))?;

        macro_rules! supported_suffixes {
            ($pasuf: ident, $suffixes: ident, $($n: literal => $e: expr),*) => {
                let $suffixes = vec![$(clap::builder::PossibleValue::new($n)),*];
                fn $pasuf(suffix: &str) -> u64 {
                    match suffix.to_ascii_lowercase().as_str() {
                        $($n => $e),*,
                        _ => unreachable!()
                    }
                }
            };
        }
        supported_suffixes!(parse_suffix, suffixes,
            "" => 1,
            "kb" => 10u64.pow(3),
            "kib" => 2u64.pow(10),
            "mb" => 10u64.pow(6),
            "mib" => 2u64.pow(20),
            "gb" => 10u64.pow(9),
            "gib" => 2u64.pow(30),
            "tb" => 10u64.pow(12),
            "tib" => 2u64.pow(40),
            "pb" => 10u64.pow(15),
            "pib" => 2u64.pow(50)
        );
        let file_size_mod = PossibleValuesParser::new(suffixes)
            .map(|suffix| parse_suffix(&suffix))
            .parse_ref(cmd, arg, rem.as_ref())?;
        let final_file_size = fs_literal
            .checked_mul(file_size_mod)
            .ok_or(ParseIntError::Overflow)
            .map_err(|err| format_int_err(err, cmd, arg))?;
        Ok(FileSize(final_file_size))
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum ParseIntError {
    Overflow,
}

fn format_int_err(err: ParseIntError, cmd: &clap::Command, arg: Option<&Arg>) -> clap::Error {
    let arg = arg.map(std::string::ToString::to_string);
    let literal_style = cmd.get_styles().get_literal();
    let for_arg_txt = if let Some(arg) = &arg {
        format!(
            " for arg '{}{arg}{}'",
            literal_style.render(),
            literal_style.render_reset()
        )
    } else {
        String::new()
    };
    let mut err = match err {
        ParseIntError::Overflow => clap::Error::raw(
            ClapErrorKind::InvalidValue,
            format!("The given file size{for_arg_txt} size is too large to be represented\n"),
        ),
    };
    if let Some(arg) = arg {
        err.insert(ClapContextKind::InvalidArg, ClapContextValue::String(arg));
    }
    err.with_cmd(cmd)
}

/// parse a number and return the remaining text
fn parse_number_prefix<'t>(
    text: &'t str,
    char_ranges: &'static [(u8, u8, u8)],
    radix: u8,
) -> Result<(u64, &'t str), ParseIntError> {
    let mut acc = 0u64;
    let mut chars = text.char_indices();

    let remaining = 'char_loop: loop {
        let Some((char_i, char)) = chars.next() else { break ""; };
        let char_byte = char.to_ascii_uppercase() as u8;
        if char == '_' {
            continue;
        }
        for (base, lower, upper) in char_ranges {
            if char_byte >= *lower && char_byte < *upper {
                acc = acc
                    .checked_mul(radix.into())
                    .and_then(|acc| acc.checked_add((*base + char_byte - *lower).into()))
                    .ok_or(ParseIntError::Overflow)?;
                continue 'char_loop;
            }
        }
        break &text[char_i..];
    };
    Ok((acc, remaining))
}

#[test]
fn test_num_parse() {
    let samples = vec![
        ("0x13", 0x13),
        ("0O012", 0o12),
        ("0b101", 0b101),
        ("011312", 11312),
        ("0xFeF", 0xfef),
        ("0x1eb", 0x1eb),
        ("0o0123kb", 0o123 * 1000),
        ("0xAFmIb", 0xAF * 2u64.pow(20)),
    ];
    let command = clap::Command::new("test")
        .arg(
            clap::Arg::new("nums")
                .action(clap::ArgAction::Append)
                .value_parser(clap::builder::ValueParser::from(FileSizeValueParser))
                .ignore_case(true),
        )
        .no_binary_name(true);
    let (strs, expected) = samples.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
    let matches = command.get_matches_from(strs);
    let sizes = matches.get_many::<FileSize>("nums").unwrap();
    assert_eq!(sizes.map(|s| s.0).collect::<Vec<_>>(), expected);
}

#[test]
fn test_num_prefix() {
    let result =
        parse_number_prefix("1923123basdjas", [(0, b'0', b'9' + 1)].as_slice(), 10).unwrap();
    assert_eq!(result, (1923123, "basdjas"));
    let result = parse_number_prefix("00", [(0, b'0', b'9' + 1)].as_slice(), 10).unwrap();
    assert_eq!(result, (0, ""));
    let result = parse_number_prefix("413lpik", [(0, b'0', b'7' + 1)].as_slice(), 8).unwrap();
    assert_eq!(result, (0o413, "lpik"));
    let result = parse_number_prefix("01012345", [(0, b'0', b'1' + 1)].as_slice(), 2).unwrap();
    assert_eq!(result, (0b0101, "2345"));
    let result = parse_number_prefix(
        "184467440737095516151basd",
        [(0, b'0', b'9' + 1)].as_slice(),
        10,
    )
    .unwrap_err();
    assert!(matches!(result, ParseIntError::Overflow));
    let hexrange = [(0, b'0', b'9' + 1), (10, b'A', b'F' + 1)].as_slice();
    let result = parse_number_prefix("9Eefx", hexrange, 16).unwrap();
    assert_eq!(result, (0x9Eef, "x"));
}
