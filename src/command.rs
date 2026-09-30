use thiserror::Error;

use crate::RespValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Ping,
    Echo(Vec<u8>),

    Get(Vec<u8>),
    Set { key: Vec<u8>, value: Vec<u8> },
    Del(Vec<Vec<u8>>),

    Exists(Vec<Vec<u8>>),

    Incr(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error("command must be an array")]
    NotArray,

    #[error("null array is not a command")]
    NullArray,

    #[error("command cannot be empty")]
    Empty,

    #[error("command name must be a bulk string")]
    InvalidCommandName,

    #[error("unknown command: {0}")]
    UnknownCommand(String),

    #[error("wrong number of arguments for {command}")]
    WrongArity { command: &'static str },

    #[error("invalid argument for {command}")]
    InvalidArgument { command: &'static str },
}

impl Command {
    pub fn from_resp(value: RespValue) -> Result<Self, CommandError> {
        let values = match value {
            RespValue::Array(Some(values)) => values,
            RespValue::Array(None) => return Err(CommandError::NullArray),
            _ => return Err(CommandError::NotArray),
        };

        let mut values = values.into_iter();

        let command = values.next().ok_or(CommandError::Empty)?;

        let command = match command {
            RespValue::BulkString(Some(command)) => command,
            _ => return Err(CommandError::InvalidCommandName),
        };

        match command.to_ascii_uppercase().as_slice() {
            b"PING" => Self::parse_ping(values),
            b"ECHO" => Self::parse_echo(values),
            b"GET" => Self::parse_get(values),
            b"SET" => Self::parse_set(values),
            b"DEL" => Self::parse_del(values),
            b"EXISTS" => Self::parse_exists(values),
            b"INCR" => Self::parse_incr(values),

            _ => {
                let command = String::from_utf8_lossy(&command).into_owned();

                Err(CommandError::UnknownCommand(command))
            },
        }
    }

    fn parse_ping(
        mut args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        if args.next().is_some() {
            return Err(CommandError::WrongArity { command: "PING" });
        }

        Ok(Self::Ping)
    }

    fn parse_echo(
        mut args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        let message = Self::bulk_arg(&mut args, "ECHO")?;

        if args.next().is_some() {
            return Err(CommandError::WrongArity { command: "ECHO" });
        }

        Ok(Self::Echo(message))
    }

    fn parse_get(
        mut args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        let key = Self::bulk_arg(&mut args, "GET")?;

        if args.next().is_some() {
            return Err(CommandError::WrongArity { command: "GET" });
        }

        Ok(Self::Get(key))
    }

    fn parse_set(
        mut args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        let key = Self::bulk_arg(&mut args, "SET")?;
        let value = Self::bulk_arg(&mut args, "SET")?;

        if args.next().is_some() {
            return Err(CommandError::WrongArity { command: "SET" });
        }

        Ok(Self::Set { key, value })
    }

    fn parse_del(
        args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        let keys = Self::bulk_args(args, "DEL")?;

        if keys.is_empty() {
            return Err(CommandError::WrongArity { command: "DEL" });
        }

        Ok(Self::Del(keys))
    }

    fn parse_exists(
        args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        let keys = Self::bulk_args(args, "EXISTS")?;

        if keys.is_empty() {
            return Err(CommandError::WrongArity { command: "EXISTS" });
        }

        Ok(Self::Exists(keys))
    }

    fn parse_incr(
        mut args: impl Iterator<Item = RespValue>,
    ) -> Result<Self, CommandError> {
        let key = Self::bulk_arg(&mut args, "INCR")?;

        if args.next().is_some() {
            return Err(CommandError::WrongArity { command: "INCR" });
        }

        Ok(Self::Incr(key))
    }

    fn bulk_arg(
        args: &mut impl Iterator<Item = RespValue>,
        command: &'static str,
    ) -> Result<Vec<u8>, CommandError> {
        match args.next() {
            Some(RespValue::BulkString(Some(value))) => Ok(value),

            Some(_) => Err(CommandError::InvalidArgument { command }),

            None => Err(CommandError::WrongArity { command }),
        }
    }

    fn bulk_args(
        args: impl Iterator<Item = RespValue>,
        command: &'static str,
    ) -> Result<Vec<Vec<u8>>, CommandError> {
        args.map(|value| match value {
            RespValue::BulkString(Some(value)) => Ok(value),

            _ => Err(CommandError::InvalidArgument { command }),
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- helpers ----------

    fn bulk(s: &[u8]) -> RespValue {
        RespValue::BulkString(Some(s.to_vec()))
    }

    fn request(parts: &[&str]) -> RespValue {
        RespValue::Array(Some(
            parts.iter().map(|p| bulk(p.as_bytes())).collect(),
        ))
    }

    fn parse(parts: &[&str]) -> Result<Command, CommandError> {
        Command::from_resp(request(parts))
    }

    fn b(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    // ---------- happy paths ----------

    #[test]
    fn parses_ping() {
        assert_eq!(parse(&["PING"]), Ok(Command::Ping));
    }

    #[test]
    fn parses_echo() {
        assert_eq!(parse(&["ECHO", "hello"]), Ok(Command::Echo(b("hello"))));
    }

    #[test]
    fn parses_get() {
        assert_eq!(parse(&["GET", "key"]), Ok(Command::Get(b("key"))));
    }

    #[test]
    fn parses_set() {
        assert_eq!(
            parse(&["SET", "key", "value"]),
            Ok(Command::Set { key: b("key"), value: b("value") })
        );
    }

    #[test]
    fn parses_del_single_key() {
        assert_eq!(parse(&["DEL", "a"]), Ok(Command::Del(vec![b("a")])));
    }

    #[test]
    fn parses_del_multiple_keys_preserving_order_and_duplicates() {
        assert_eq!(
            parse(&["DEL", "a", "b", "a"]),
            Ok(Command::Del(vec![b("a"), b("b"), b("a")]))
        );
    }

    #[test]
    fn parses_exists_single_key() {
        assert_eq!(parse(&["EXISTS", "a"]), Ok(Command::Exists(vec![b("a")])));
    }

    #[test]
    fn parses_exists_multiple_keys() {
        assert_eq!(
            parse(&["EXISTS", "a", "b", "c"]),
            Ok(Command::Exists(vec![b("a"), b("b"), b("c")]))
        );
    }

    #[test]
    fn parses_incr() {
        assert_eq!(
            parse(&["INCR", "counter"]),
            Ok(Command::Incr(b("counter")))
        );
    }

    // ---------- case-insensitivity ----------

    #[test]
    fn command_names_are_case_insensitive() {
        assert_eq!(parse(&["ping"]), Ok(Command::Ping));
        assert_eq!(parse(&["PiNg"]), Ok(Command::Ping));
        assert_eq!(parse(&["get", "k"]), Ok(Command::Get(b("k"))));
        assert_eq!(parse(&["gEt", "k"]), Ok(Command::Get(b("k"))));
        assert_eq!(parse(&["incr", "k"]), Ok(Command::Incr(b("k"))));
    }

    #[test]
    fn arguments_are_not_case_folded() {
        assert_eq!(parse(&["get", "MyKey"]), Ok(Command::Get(b("MyKey"))));
    }

    // ---------- binary safety ----------

    #[test]
    fn keys_and_values_may_be_arbitrary_bytes() {
        let key = vec![0x00, 0xff, 0xfe, b'\r', b'\n'];
        let value = vec![0x80, 0x00, 0x7f];

        let req = RespValue::Array(Some(vec![
            bulk(b"SET"),
            bulk(&key),
            bulk(&value),
        ]));

        assert_eq!(Command::from_resp(req), Ok(Command::Set { key, value }));
    }

    #[test]
    fn empty_bulk_string_arguments_are_valid() {
        assert_eq!(parse(&["GET", ""]), Ok(Command::Get(vec![])));
        assert_eq!(
            parse(&["SET", "", ""]),
            Ok(Command::Set { key: vec![], value: vec![] })
        );
    }

    // ---------- structural errors ----------

    #[test]
    fn non_array_is_rejected() {
        let value = bulk(b"PING");
        assert_eq!(Command::from_resp(value), Err(CommandError::NotArray));
    }

    #[test]
    fn null_bulk_string_at_top_level_is_not_an_array() {
        let value = RespValue::BulkString(None);
        assert_eq!(Command::from_resp(value), Err(CommandError::NotArray));
    }

    #[test]
    fn null_array_is_rejected() {
        assert_eq!(
            Command::from_resp(RespValue::Array(None)),
            Err(CommandError::NullArray)
        );
    }

    #[test]
    fn empty_array_is_rejected() {
        assert_eq!(
            Command::from_resp(RespValue::Array(Some(vec![]))),
            Err(CommandError::Empty)
        );
    }

    #[test]
    fn null_bulk_string_as_command_name_is_rejected() {
        let req = RespValue::Array(Some(vec![RespValue::BulkString(None)]));
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidCommandName)
        );
    }

    #[test]
    fn non_bulk_command_name_is_rejected() {
        let req = RespValue::Array(Some(vec![RespValue::Array(None)]));
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidCommandName)
        );
    }

    // ---------- unknown commands ----------

    #[test]
    fn unknown_command_reports_its_name() {
        assert_eq!(
            parse(&["FLUSHALL"]),
            Err(CommandError::UnknownCommand("FLUSHALL".to_string()))
        );
    }

    #[test]
    fn unknown_command_preserves_original_casing() {
        assert_eq!(
            parse(&["FooBar"]),
            Err(CommandError::UnknownCommand("FooBar".to_string()))
        );
    }

    #[test]
    fn unknown_command_with_invalid_utf8_is_lossy() {
        let req = RespValue::Array(Some(vec![bulk(&[0xff, 0xfe])]));
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::UnknownCommand("\u{fffd}\u{fffd}".to_string()))
        );
    }

    #[test]
    fn unknown_command_error_message() {
        let err = parse(&["NOPE"]).unwrap_err();
        assert_eq!(err.to_string(), "unknown command: NOPE");
    }

    // ---------- arity errors ----------

    #[test]
    fn ping_rejects_extra_arguments() {
        assert_eq!(
            parse(&["PING", "extra"]),
            Err(CommandError::WrongArity { command: "PING" })
        );
    }

    #[test]
    fn echo_requires_exactly_one_argument() {
        assert_eq!(
            parse(&["ECHO"]),
            Err(CommandError::WrongArity { command: "ECHO" })
        );
        assert_eq!(
            parse(&["ECHO", "a", "b"]),
            Err(CommandError::WrongArity { command: "ECHO" })
        );
    }

    #[test]
    fn get_requires_exactly_one_argument() {
        assert_eq!(
            parse(&["GET"]),
            Err(CommandError::WrongArity { command: "GET" })
        );
        assert_eq!(
            parse(&["GET", "a", "b"]),
            Err(CommandError::WrongArity { command: "GET" })
        );
    }

    #[test]
    fn set_requires_exactly_two_arguments() {
        assert_eq!(
            parse(&["SET"]),
            Err(CommandError::WrongArity { command: "SET" })
        );
        assert_eq!(
            parse(&["SET", "key"]),
            Err(CommandError::WrongArity { command: "SET" })
        );
        assert_eq!(
            parse(&["SET", "key", "value", "extra"]),
            Err(CommandError::WrongArity { command: "SET" })
        );
    }

    #[test]
    fn set_with_redis_options_is_currently_rejected() {
        // Documents current behaviour: options like EX/NX are not supported yet.
        assert_eq!(
            parse(&["SET", "key", "value", "EX", "10"]),
            Err(CommandError::WrongArity { command: "SET" })
        );
    }

    #[test]
    fn del_requires_at_least_one_key() {
        assert_eq!(
            parse(&["DEL"]),
            Err(CommandError::WrongArity { command: "DEL" })
        );
    }

    #[test]
    fn exists_requires_at_least_one_key() {
        assert_eq!(
            parse(&["EXISTS"]),
            Err(CommandError::WrongArity { command: "EXISTS" })
        );
    }

    #[test]
    fn incr_requires_exactly_one_argument() {
        assert_eq!(
            parse(&["INCR"]),
            Err(CommandError::WrongArity { command: "INCR" })
        );
        assert_eq!(
            parse(&["INCR", "a", "b"]),
            Err(CommandError::WrongArity { command: "INCR" })
        );
    }

    // ---------- invalid argument errors ----------

    fn with_args(name: &str, args: Vec<RespValue>) -> RespValue {
        let mut items = vec![bulk(name.as_bytes())];
        items.extend(args);
        RespValue::Array(Some(items))
    }

    #[test]
    fn echo_rejects_null_bulk_argument() {
        let req = with_args("ECHO", vec![RespValue::BulkString(None)]);
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "ECHO" })
        );
    }

    #[test]
    fn get_rejects_non_bulk_argument() {
        let req = with_args("GET", vec![RespValue::Array(None)]);
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "GET" })
        );
    }

    #[test]
    fn set_rejects_non_bulk_key() {
        let req =
            with_args("SET", vec![RespValue::Array(None), bulk(b"value")]);
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "SET" })
        );
    }

    #[test]
    fn set_rejects_non_bulk_value() {
        let req =
            with_args("SET", vec![bulk(b"key"), RespValue::BulkString(None)]);
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "SET" })
        );
    }

    #[test]
    fn incr_rejects_non_bulk_argument() {
        let req = with_args("INCR", vec![RespValue::BulkString(None)]);
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "INCR" })
        );
    }

    #[test]
    fn del_rejects_any_non_bulk_key() {
        let req = with_args(
            "DEL",
            vec![bulk(b"a"), RespValue::BulkString(None), bulk(b"c")],
        );
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "DEL" })
        );
    }

    #[test]
    fn exists_rejects_any_non_bulk_key() {
        let req = with_args("EXISTS", vec![RespValue::Array(None), bulk(b"b")]);
        assert_eq!(
            Command::from_resp(req),
            Err(CommandError::InvalidArgument { command: "EXISTS" })
        );
    }

    // ---------- error messages ----------

    #[test]
    fn error_display_messages() {
        assert_eq!(
            CommandError::NotArray.to_string(),
            "command must be an array"
        );
        assert_eq!(
            CommandError::NullArray.to_string(),
            "null array is not a command"
        );
        assert_eq!(CommandError::Empty.to_string(), "command cannot be empty");
        assert_eq!(
            CommandError::InvalidCommandName.to_string(),
            "command name must be a bulk string"
        );
        assert_eq!(
            CommandError::WrongArity { command: "GET" }.to_string(),
            "wrong number of arguments for GET"
        );
        assert_eq!(
            CommandError::InvalidArgument { command: "SET" }.to_string(),
            "invalid argument for SET"
        );
    }
}
