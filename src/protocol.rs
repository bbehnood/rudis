use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<Vec<u8>>),
    Array(Option<Vec<RespValue>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("incomplete frame")]
    Incomplete,
    #[error("invalid type byte: {0}")]
    InvalidType(u8),
    #[error("invalid integer")]
    InvalidInteger,
    #[error("invalid length")]
    InvalidLength,
    #[error("invalid utf-8 in simple string/error")]
    InvalidUtf8,
    #[error("expected CRLF")]
    MissingCrlf,
    #[error("frame exceeds size limit")]
    TooLarge,
    #[error("frame nested too deeply")]
    TooDeep,
}

pub struct RespParser<'a> {
    buf: &'a [u8],
    pos: usize,
}

pub const MAX_BULK_LEN: usize = 512 * 1024 * 1024;
pub const MAX_ARRAY_LEN: usize = 1024 * 1024;
pub const MAX_DEPTH: usize = 32;

impl<'a> RespParser<'a> {
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn parse(&mut self) -> Result<(RespValue, usize), ParseError> {
        let value = self.parse_value(0)?;
        Ok((value, self.pos))
    }

    fn parse_value(&mut self, depth: usize) -> Result<RespValue, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::TooDeep);
        }

        let ty = self.read_byte()?;

        match ty {
            b'+' => self.parse_simple_string(),
            b'-' => self.parse_error(),
            b':' => self.parse_integer(),
            b'$' => self.parse_bulk_string(),
            b'*' => self.parse_array(depth),
            other => Err(ParseError::InvalidType(other)),
        }
    }

    fn parse_simple_string(&mut self) -> Result<RespValue, ParseError> {
        let line = self.read_line()?;

        let s =
            std::str::from_utf8(line).map_err(|_| ParseError::InvalidUtf8)?;

        Ok(RespValue::SimpleString(s.to_owned()))
    }

    fn parse_error(&mut self) -> Result<RespValue, ParseError> {
        let line = self.read_line()?;

        let s =
            std::str::from_utf8(line).map_err(|_| ParseError::InvalidUtf8)?;

        Ok(RespValue::Error(s.to_owned()))
    }

    fn parse_integer(&mut self) -> Result<RespValue, ParseError> {
        let line = self.read_line()?;

        let s = std::str::from_utf8(line)
            .map_err(|_| ParseError::InvalidInteger)?;

        let value = s.parse::<i64>().map_err(|_| ParseError::InvalidInteger)?;

        Ok(RespValue::Integer(value))
    }

    fn parse_bulk_string(&mut self) -> Result<RespValue, ParseError> {
        let line = self.read_line()?;

        let len_str =
            std::str::from_utf8(line).map_err(|_| ParseError::InvalidLength)?;

        let len = len_str
            .parse::<i64>()
            .map_err(|_| ParseError::InvalidLength)?;

        if len == -1 {
            return Ok(RespValue::BulkString(None));
        }

        if len < 0 {
            return Err(ParseError::InvalidLength);
        }

        let len =
            usize::try_from(len).map_err(|_| ParseError::InvalidLength)?;

        if len > MAX_BULK_LEN {
            return Err(ParseError::TooLarge);
        }

        let end = self
            .pos
            .checked_add(len)
            .and_then(|x| x.checked_add(2))
            .ok_or(ParseError::InvalidLength)?;

        if end > self.buf.len() {
            return Err(ParseError::Incomplete);
        }

        let payload = self.buf[self.pos..self.pos + len].to_vec();

        self.pos += len;

        self.expect_crlf()?;

        Ok(RespValue::BulkString(Some(payload)))
    }

    fn parse_array(&mut self, depth: usize) -> Result<RespValue, ParseError> {
        let line = self.read_line()?;

        let len_str =
            std::str::from_utf8(line).map_err(|_| ParseError::InvalidLength)?;

        let len = len_str
            .parse::<i64>()
            .map_err(|_| ParseError::InvalidLength)?;

        if len == -1 {
            return Ok(RespValue::Array(None));
        }

        if len < 0 {
            return Err(ParseError::InvalidLength);
        }

        let len =
            usize::try_from(len).map_err(|_| ParseError::InvalidLength)?;

        if len > MAX_ARRAY_LEN {
            return Err(ParseError::TooLarge);
        }

        let mut values = Vec::with_capacity(len);

        for _ in 0..len {
            values.push(self.parse_value(depth + 1)?);
        }

        Ok(RespValue::Array(Some(values)))
    }

    fn read_byte(&mut self) -> Result<u8, ParseError> {
        if self.pos >= self.buf.len() {
            return Err(ParseError::Incomplete);
        }

        let byte = self.buf[self.pos];
        self.pos += 1;

        Ok(byte)
    }

    fn read_line(&mut self) -> Result<&'a [u8], ParseError> {
        let start = self.pos;

        while self.pos + 1 < self.buf.len() {
            if self.buf[self.pos] == b'\r' && self.buf[self.pos + 1] == b'\n' {
                let line = &self.buf[start..self.pos];
                self.pos += 2;
                return Ok(line);
            }

            self.pos += 1;
        }

        Err(ParseError::Incomplete)
    }

    fn expect_crlf(&mut self) -> Result<(), ParseError> {
        if self.pos + 1 >= self.buf.len() {
            return Err(ParseError::Incomplete);
        }

        if self.buf[self.pos] != b'\r' || self.buf[self.pos + 1] != b'\n' {
            return Err(ParseError::MissingCrlf);
        }

        self.pos += 2;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[u8]) -> Result<(RespValue, usize), ParseError> {
        RespParser::new(input).parse()
    }

    fn ok(input: &[u8]) -> RespValue {
        let (v, consumed) = parse(input).expect("expected successful parse");
        assert_eq!(consumed, input.len(), "should consume entire input");
        v
    }

    fn bulk(s: &str) -> RespValue {
        RespValue::BulkString(Some(s.as_bytes().to_vec()))
    }

    // ---------- Simple strings ----------

    #[test]
    fn simple_string() {
        assert_eq!(ok(b"+OK\r\n"), RespValue::SimpleString("OK".into()));
    }

    #[test]
    fn simple_string_empty() {
        assert_eq!(ok(b"+\r\n"), RespValue::SimpleString(String::new()));
    }

    #[test]
    fn simple_string_with_spaces_and_unicode() {
        assert_eq!(
            ok("+héllo wörld\r\n".as_bytes()),
            RespValue::SimpleString("héllo wörld".into())
        );
    }

    #[test]
    fn simple_string_invalid_utf8() {
        assert_eq!(parse(b"+\xff\xfe\r\n"), Err(ParseError::InvalidUtf8));
    }

    #[test]
    fn simple_string_bare_cr_or_lf_is_not_terminator() {
        // A lone \n or \r does not terminate the line.
        assert_eq!(parse(b"+OK\n"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"+OK\r"), Err(ParseError::Incomplete));
    }

    #[test]
    fn simple_string_embedded_cr_preserved() {
        assert_eq!(ok(b"+a\rb\r\n"), RespValue::SimpleString("a\rb".into()));
    }

    // ---------- Errors ----------

    #[test]
    fn error_value() {
        assert_eq!(
            ok(b"-ERR unknown command\r\n"),
            RespValue::Error("ERR unknown command".into())
        );
    }

    #[test]
    fn error_invalid_utf8() {
        assert_eq!(parse(b"-\xc3\x28\r\n"), Err(ParseError::InvalidUtf8));
    }

    // ---------- Integers ----------

    #[test]
    fn integer_positive_negative_zero() {
        assert_eq!(ok(b":1000\r\n"), RespValue::Integer(1000));
        assert_eq!(ok(b":-42\r\n"), RespValue::Integer(-42));
        assert_eq!(ok(b":0\r\n"), RespValue::Integer(0));
        assert_eq!(ok(b":+7\r\n"), RespValue::Integer(7));
    }

    #[test]
    fn integer_bounds() {
        assert_eq!(
            ok(b":9223372036854775807\r\n"),
            RespValue::Integer(i64::MAX)
        );
        assert_eq!(
            ok(b":-9223372036854775808\r\n"),
            RespValue::Integer(i64::MIN)
        );
    }

    #[test]
    fn integer_overflow() {
        assert_eq!(
            parse(b":9223372036854775808\r\n"),
            Err(ParseError::InvalidInteger)
        );
    }

    #[test]
    fn integer_invalid() {
        for input in [
            &b":\r\n"[..],
            b":abc\r\n",
            b":12a\r\n",
            b":1 2\r\n",
            b": 1\r\n",
            b":1.5\r\n",
            b":\xff\r\n",
        ] {
            assert_eq!(
                parse(input),
                Err(ParseError::InvalidInteger),
                "input: {:?}",
                String::from_utf8_lossy(input)
            );
        }
    }

    // ---------- Bulk strings ----------

    #[test]
    fn bulk_string_basic() {
        assert_eq!(ok(b"$5\r\nhello\r\n"), bulk("hello"));
    }

    #[test]
    fn bulk_string_empty() {
        assert_eq!(ok(b"$0\r\n\r\n"), RespValue::BulkString(Some(vec![])));
    }

    #[test]
    fn bulk_string_null() {
        assert_eq!(ok(b"$-1\r\n"), RespValue::BulkString(None));
    }

    #[test]
    fn bulk_string_binary_safe() {
        assert_eq!(
            ok(b"$6\r\n\x00\xff\r\n\x01\x02\r\n"),
            RespValue::BulkString(Some(vec![
                0x00, 0xff, b'\r', b'\n', 0x01, 0x02
            ]))
        );
    }

    #[test]
    fn bulk_string_payload_containing_crlf() {
        assert_eq!(ok(b"$4\r\na\r\nb\r\n"), bulk("a\r\nb"));
    }

    #[test]
    fn bulk_string_consumed_count_with_trailing_data() {
        let input = b"$3\r\nfoo\r\n+extra\r\n";
        let (v, consumed) = parse(input).unwrap();
        assert_eq!(v, bulk("foo"));
        assert_eq!(consumed, 9);
    }

    #[test]
    fn bulk_string_missing_crlf() {
        assert_eq!(parse(b"$3\r\nfooXY"), Err(ParseError::MissingCrlf));
    }

    #[test]
    fn bulk_string_length_mismatch_too_long_payload() {
        // Declared 3 but 5 bytes of payload precede the CRLF.
        assert_eq!(parse(b"$3\r\nhello\r\n"), Err(ParseError::MissingCrlf));
    }

    #[test]
    fn bulk_string_truncated_payload() {
        assert_eq!(parse(b"$5\r\nhel"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"$5\r\nhello"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"$5\r\nhello\r"), Err(ParseError::Incomplete));
    }

    #[test]
    fn bulk_string_invalid_length() {
        for input in [
            &b"$abc\r\n"[..],
            b"$\r\n",
            b"$-2\r\n",
            b"$-100\r\n",
            b"$1.5\r\n",
            b"$\xff\r\n",
        ] {
            assert_eq!(
                parse(input),
                Err(ParseError::InvalidLength),
                "input: {:?}",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn bulk_string_too_large() {
        let input = format!("${}\r\n", MAX_BULK_LEN + 1);
        assert_eq!(parse(input.as_bytes()), Err(ParseError::TooLarge));
    }

    #[test]
    fn bulk_string_at_limit_is_incomplete_not_too_large() {
        let input = format!("${}\r\n", MAX_BULK_LEN);
        assert_eq!(parse(input.as_bytes()), Err(ParseError::Incomplete));
    }

    #[test]
    fn bulk_string_huge_length_does_not_panic() {
        // Larger than i64: fails integer parse.
        assert_eq!(
            parse(b"$99999999999999999999\r\n"),
            Err(ParseError::InvalidLength)
        );
        assert_eq!(
            parse(b"$9223372036854775807\r\n"),
            Err(ParseError::TooLarge)
        );
    }

    // ---------- Arrays ----------

    #[test]
    fn array_empty() {
        assert_eq!(ok(b"*0\r\n"), RespValue::Array(Some(vec![])));
    }

    #[test]
    fn array_null() {
        assert_eq!(ok(b"*-1\r\n"), RespValue::Array(None));
    }

    #[test]
    fn array_of_bulk_strings() {
        assert_eq!(
            ok(b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n"),
            RespValue::Array(Some(vec![bulk("foo"), bulk("bar")]))
        );
    }

    #[test]
    fn array_mixed_types() {
        assert_eq!(
            ok(b"*5\r\n:1\r\n+two\r\n-three\r\n$4\r\nfour\r\n$-1\r\n"),
            RespValue::Array(Some(vec![
                RespValue::Integer(1),
                RespValue::SimpleString("two".into()),
                RespValue::Error("three".into()),
                bulk("four"),
                RespValue::BulkString(None),
            ]))
        );
    }

    #[test]
    fn array_nested() {
        assert_eq!(
            ok(b"*2\r\n*2\r\n:1\r\n:2\r\n*1\r\n*0\r\n"),
            RespValue::Array(Some(vec![
                RespValue::Array(Some(vec![
                    RespValue::Integer(1),
                    RespValue::Integer(2)
                ])),
                RespValue::Array(Some(vec![RespValue::Array(Some(vec![]))])),
            ]))
        );
    }

    #[test]
    fn array_containing_null_array() {
        assert_eq!(
            ok(b"*1\r\n*-1\r\n"),
            RespValue::Array(Some(vec![RespValue::Array(None)]))
        );
    }

    #[test]
    fn array_typical_redis_command() {
        assert_eq!(
            ok(b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"),
            RespValue::Array(Some(vec![
                bulk("SET"),
                bulk("key"),
                bulk("value")
            ]))
        );
    }

    #[test]
    fn array_invalid_length() {
        for input in [&b"*abc\r\n"[..], b"*\r\n", b"*-2\r\n", b"*1.0\r\n"] {
            assert_eq!(
                parse(input),
                Err(ParseError::InvalidLength),
                "input: {:?}",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn array_too_large() {
        let input = format!("*{}\r\n", MAX_ARRAY_LEN + 1);
        assert_eq!(parse(input.as_bytes()), Err(ParseError::TooLarge));
    }

    #[test]
    fn array_fewer_elements_than_declared_is_incomplete() {
        assert_eq!(parse(b"*3\r\n:1\r\n:2\r\n"), Err(ParseError::Incomplete));
    }

    #[test]
    fn array_element_error_propagates() {
        assert_eq!(
            parse(b"*2\r\n:1\r\n?bad\r\n"),
            Err(ParseError::InvalidType(b'?'))
        );
        assert_eq!(
            parse(b"*2\r\n:1\r\n:xyz\r\n"),
            Err(ParseError::InvalidInteger)
        );
    }

    #[test]
    fn array_trailing_data_not_consumed() {
        let input = b"*1\r\n:1\r\n:2\r\n";
        let (v, consumed) = parse(input).unwrap();
        assert_eq!(v, RespValue::Array(Some(vec![RespValue::Integer(1)])));
        assert_eq!(consumed, 8);
    }

    // ---------- Depth limit ----------

    fn nested(levels: usize) -> Vec<u8> {
        // `levels` nested single-element arrays, innermost is an empty array.
        let mut buf = Vec::new();
        for _ in 0..levels {
            buf.extend_from_slice(b"*1\r\n");
        }
        buf.extend_from_slice(b"*0\r\n");
        buf
    }

    #[test]
    fn depth_at_limit_ok() {
        // Outer array is depth 0; innermost empty array sits at MAX_DEPTH.
        let buf = nested(MAX_DEPTH);
        assert!(parse(&buf).is_ok());
    }

    #[test]
    fn depth_over_limit_rejected() {
        let buf = nested(MAX_DEPTH + 1);
        assert_eq!(parse(&buf), Err(ParseError::TooDeep));
    }

    #[test]
    fn very_deep_nesting_does_not_overflow_stack() {
        let buf = nested(100_000);
        assert_eq!(parse(&buf), Err(ParseError::TooDeep));
    }

    // ---------- Type byte / framing ----------

    #[test]
    fn empty_input_is_incomplete() {
        assert_eq!(parse(b""), Err(ParseError::Incomplete));
    }

    #[test]
    fn invalid_type_byte() {
        assert_eq!(parse(b"?foo\r\n"), Err(ParseError::InvalidType(b'?')));
        assert_eq!(parse(b"\r\n"), Err(ParseError::InvalidType(b'\r')));
        assert_eq!(parse(b"\x00"), Err(ParseError::InvalidType(0)));
    }

    #[test]
    fn type_byte_only_is_incomplete() {
        for input in [&b"+"[..], b"-", b":", b"$", b"*"] {
            assert_eq!(parse(input), Err(ParseError::Incomplete));
        }
    }

    #[test]
    fn every_strict_prefix_of_a_frame_is_incomplete() {
        let frame = b"*3\r\n$3\r\nSET\r\n:42\r\n+OK\r\n";
        for i in 0..frame.len() {
            assert_eq!(
                parse(&frame[..i]),
                Err(ParseError::Incomplete),
                "prefix length {i}"
            );
        }
        assert!(parse(frame).is_ok());
    }

    #[test]
    fn pipelined_frames_parse_sequentially() {
        let input = b"+OK\r\n:5\r\n$2\r\nhi\r\n";
        let mut offset = 0;
        let mut out = Vec::new();
        while offset < input.len() {
            let (v, n) = parse(&input[offset..]).unwrap();
            out.push(v);
            offset += n;
        }
        assert_eq!(
            out,
            vec![
                RespValue::SimpleString("OK".into()),
                RespValue::Integer(5),
                bulk("hi"),
            ]
        );
    }

    #[test]
    fn error_display_messages() {
        assert_eq!(ParseError::Incomplete.to_string(), "incomplete frame");
        assert_eq!(
            ParseError::InvalidType(b'?').to_string(),
            "invalid type byte: 63"
        );
        assert_eq!(ParseError::TooDeep.to_string(), "frame nested too deeply");
    }
}
