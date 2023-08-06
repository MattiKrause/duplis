use std::ffi::OsStr;
use std::marker::PhantomData;
use std::num::{IntErrorKind, ParseIntError};
use clap::builder::TypedValueParser;
use clap::{Arg, Command, Error};
use clap::error::{ContextKind, ContextValue};

#[derive(Clone, Debug)]
pub struct UNumberParser<T>(PhantomData<T>);

impl UNumberParser<u64> {
    pub fn u64() -> Self {
        Self(PhantomData)
    }
}

impl UNumberParser<u32> {
    pub fn u32() -> Self {
        Self(PhantomData)
    }
}

fn invalid_digit_error(cmd: &clap::Command, arg: Option<&clap::Arg>) -> clap::Error {
    let arg_text = arg.map_or(String::new(), |arg| {
        let literal = cmd.get_styles().get_literal();
        format!(" in arg '{}{arg}{}'", literal.render(), literal.render_reset())
    });

    clap::Error::raw(clap::error::ErrorKind::InvalidValue, format!("invalid number{arg_text}: invalid digit"))
}

fn overflow_error(cmd: &clap::Command, arg: Option<&clap::Arg>) -> clap::Error {
    let arg_text = arg.map_or(String::new(), |arg| {
        let literal = cmd.get_styles().get_literal();
        format!(" in arg '{}{arg}{}'", literal.render(), literal.render_reset())
    });

    clap::Error::raw(clap::error::ErrorKind::InvalidValue, format!("invalid number{arg_text}: number too large"))
}

impl <T> UNumberParser<T> {

    fn _parse_ref(&self, cmd: &clap::Command, arg: Option<&clap::Arg>, value: &OsStr) -> Result<u64, clap::Error> {
        let mut str = value.to_str().ok_or_else(|| clap::Error::new(clap::error::ErrorKind::InvalidUtf8))?;
        let radix = if str.len() >= 2 && str.starts_with('0') {
            str = &str[1..];
            let char = str.chars().next().unwrap();
            str = &str[1..];
            match char {
                'b' | 'B' => 2,
                'o' | 'O' => 8,
                'x' | 'X' => 16,
                _ => return Err(invalid_digit_error(cmd, arg))
            }
        } else {
            10
        };

        match u64::from_str_radix(str, radix).map_err(|e| e.kind().clone()) {
            Ok(v) => Ok(v),
            Err(IntErrorKind::Empty) => Ok(0),
            Err(IntErrorKind::InvalidDigit) => Err(invalid_digit_error(cmd, arg)),
            Err(IntErrorKind::PosOverflow) => Err(overflow_error(cmd, arg)),
            Err(IntErrorKind::NegOverflow | IntErrorKind::Zero) => unreachable!(),
            Err(_) => Err(invalid_digit_error(cmd, arg))
        }
    }
}

impl TypedValueParser for UNumberParser<u64> {
    type Value = u64;

    fn parse_ref(&self, cmd: &Command, arg: Option<&Arg>, value: &OsStr) -> Result<Self::Value, Error> {
        self._parse_ref(cmd, arg, value)
    }
}

impl TypedValueParser for UNumberParser<u32> {
    type Value = u32;

    fn parse_ref(&self, cmd: &Command, arg: Option<&Arg>, value: &OsStr) -> Result<Self::Value, Error> {
        let value: u64 = self._parse_ref(cmd, arg, value)?;
        u32::try_from(value).map_err(|_| overflow_error(cmd, arg))
    }
}