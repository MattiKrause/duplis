use std::ffi::OsStr;
use clap::builder::{PossibleValuesParser, StringValueParser, TypedValueParser};
use clap::{Arg, Command};
use clap::error::{ErrorKind as ClapErrorKind, ContextKind as ClapContextKind, ContextValue as ClapContextValue};

#[derive(Clone)]
pub(crate) struct FileSize(pub u64);

/// Parse file size in binary/octal/hexadecimal with '_' separators and scale KB to EB and KiB to EiB
#[derive(Clone)]
pub(crate) struct FileSizeValueParser;
impl TypedValueParser for FileSizeValueParser {
    type Value = FileSize;

    fn parse_ref(&self, cmd: &Command, arg: Option<&Arg>, value: &OsStr) -> Result<Self::Value, clap::Error> {
        let value = StringValueParser::new().parse_ref(cmd, arg, value)?;

        let (fs_literal, rem, parsed) = if value.starts_with("0b") | value.starts_with("0B") {
            let (acc, rem) = parse_number_prefix(&value[2..], 2).map_err(|err| format_int_err(err, cmd, arg))?;
            (acc, rem, None)
        } else if value.starts_with("0x") | value.starts_with("0X") {
            partial_parse_hexadecimal_filesize(&value[2..]).map_err(|err| format_int_err(err, cmd, arg))?
        } else if value.starts_with("0o") | value.starts_with("0O"){
            let (acc, rem) = parse_number_prefix(&value[2..], 8).map_err(|err| format_int_err(err, cmd, arg))?;
            (acc, rem, None)
        } else {
            let (acc, rem) = parse_number_prefix(&value, 10).map_err(|err| format_int_err(err, cmd, arg))?;
            (acc, rem, None)
        };
        macro_rules! supported_suffixes {
            ($pasuf: ident, $suffixes: ident, $($n: literal => $e: expr),*) => {
                let $suffixes = vec![$(clap::builder::PossibleValue::new($n)),*];
                fn $pasuf(suffix: &str) -> u64 {
                    match suffix {
                        $($n => $e),*,
                        _ => unreachable!()
                    }
                }
            };
        }
        let file_size_mod = if let Some(parsed)= parsed {
            parsed
        } else if rem == "" {
            1
        } else {
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
                "pib" => 2u64.pow(50),
                "eb" => 10u64.pow(18),
                "eib" => 2u64.pow(60)
            );
            PossibleValuesParser::new(suffixes)
                .map(|suffix| parse_suffix(&suffix))
                .parse_ref(cmd, arg, rem.as_ref())?
        };
        let final_file_size = fs_literal.checked_mul(file_size_mod)
            .ok_or(ParseIntError::Overflow)
            .map_err(|err| format_int_err(err, cmd, arg))?;
        Ok(FileSize(final_file_size))
    }
}

enum ParseIntError {
    Overflow
}

fn format_int_err(err: ParseIntError, cmd: &Command, arg: Option<&Arg>) -> clap::Error {
    let arg = arg.map(|arg| arg.to_string());
    let literal_style = cmd.get_styles().get_literal();
    let for_arg_txt = if let Some(arg) = &arg {
        format!(" for arg '{}{arg}{}'", literal_style.render(), literal_style.render_reset())
    } else {
        String::new()
    };
    let mut err = match err {
        ParseIntError::Overflow => clap::Error::raw(ClapErrorKind::InvalidValue, format!("The given file size{for_arg_txt} size is too large to be represented\n"))
    };
    if let Some(arg) = arg {
        err.insert(ClapContextKind::InvalidArg, ClapContextValue::String(arg));
    }
    err.with_cmd(cmd)
}

/// parse a number and return the remaining text
fn parse_number_prefix(text: &str, radix: u8) -> Result<(u64, &str), ParseIntError> {
    assert!(radix > 1);
    assert!(radix <= 10);

    let mut acc = 0u64;
    let mut chars = text.char_indices();

    let remaining = loop {
        let Some((char_i, char)) = chars.next() else { break "" };
        let charb = char as u8;
        let char_u_bound = b'0' + radix - 1;
        if charb >= b'0' && charb <= char_u_bound {
            acc = acc
                .checked_mul(radix as u64)
                .and_then(|acc| acc.checked_add(char  as u64 - b'0' as u64))
                .ok_or(ParseIntError::Overflow)?;
        } else if char != '_'{
            break &text[char_i..]
        }
    };
    Ok((acc, remaining))
}

/// parse a number and maybe parse a scale and return the remaining text.
/// The parsed scale is necessary since Exabyte is supported. This means 0xEEB cannot be parsed eagerly, as is would result in '0xEE' and 'B'
/// thus we need check for these cases. In the above example '0xEE', 'B', with scale EB would be returned
fn partial_parse_hexadecimal_filesize(value: &str) -> Result<(u64, &str, Option<u64>), ParseIntError> {
    let (mut acc, remaining, last_e)= parse_hexadecimal(value)?;
    if last_e && remaining == "" {
        acc = acc.checked_mul(16).and_then(|acc| acc.checked_add(14)).ok_or(ParseIntError::Overflow)?;
    }
    let parsed = if remaining != "" && last_e {
        match remaining {
            //exbibyte
            "ib" | "iB" | "Ib" | "IB" => Some(2 << 60),
            //exabyte
            "b" | "B" => Some(10u64.pow(18)),
            _ => {
                acc = acc.checked_mul(16).and_then(|acc| acc.checked_add(14)).ok_or(ParseIntError::Overflow)?;
                None
            }
        }
    } else {
        None
    };
    Ok((acc, remaining, parsed))
}

/// parse a number, return the remaining text and whether the number ended on 'e'
fn parse_hexadecimal(value: &str) -> Result<(u64, &str, bool), ParseIntError> {
    let mut last_e = false;
    let mut chars = value.char_indices();
    let mut acc = 0u64;
    let remaining = loop {
        let Some((char_i, mut char)) = chars.next() else { break "" };
        char = char.to_ascii_uppercase();

        if ('0'..='9').contains(&char) | ('A'..='F').contains(&char){
            if last_e {
                acc = acc.checked_mul(16)
                    .and_then(|acc| acc.checked_add(14))
                    .ok_or(ParseIntError::Overflow)?;
            }
            last_e = char == 'E';
            if !last_e {
                let add_amount = match char {
                    '0'..='9' => char as u8 - b'0',
                    'A'..='F' => char as u8 - b'A',
                    _ => unreachable!()
                };
                acc = acc.checked_mul(16)
                    .and_then(|acc| acc.checked_add(add_amount as u64))
                    .ok_or(ParseIntError::Overflow)?
            }
        } else if char != '_' {
            break &value[char_i..]
        }
    };
    Ok((acc, remaining, last_e))
}