use bytes::Bytes;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<Bytes>),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EncodeError {
    #[error("simple string/error contains CR or LF")]
    InvalidString,
    #[error("value exceeds size limit")]
    TooLarge,
    #[error("value nested too deeply")]
    TooDeep,
}

pub struct RespParser<'a> {
    buf: &'a [u8],
    pos: usize,
}

pub const MAX_BULK_LEN: usize = 512 * 1024 * 1024;
pub const MAX_ARRAY_LEN: usize = 1024 * 1024;
pub const MAX_LINE_LEN: usize = 64 * 1024;
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

        let payload =
            Bytes::copy_from_slice(&self.buf[self.pos..self.pos + len]);

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

        let remaining = self.buf.len() - self.pos;
        let mut values = Vec::with_capacity(len.min(remaining / 3));

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

        let limit = start.saturating_add(MAX_LINE_LEN + 2);
        let window = &self.buf[start..self.buf.len().min(limit)];

        match window.windows(2).position(|w| w == b"\r\n") {
            Some(i) => {
                self.pos = start + i + 2;
                Ok(&self.buf[start..start + i])
            },

            None if window.len() >= MAX_LINE_LEN + 2 => {
                Err(ParseError::TooLarge)
            },

            None => Err(ParseError::Incomplete),
        }
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

impl RespValue {
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let start = out.len();
        let res = self.encode_value(out, 0);
        if res.is_err() {
            out.truncate(start);
        }
        res
    }

    fn encode_value(
        &self,
        out: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), EncodeError> {
        if depth > MAX_DEPTH {
            return Err(EncodeError::TooDeep);
        }

        match self {
            Self::SimpleString(s) => Self::encode_line(out, b'+', s)?,
            Self::Error(s) => Self::encode_line(out, b'-', s)?,
            Self::Integer(n) => {
                out.push(b':');
                out.extend_from_slice(n.to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
            },
            Self::BulkString(None) => out.extend_from_slice(b"$-1\r\n"),
            Self::BulkString(Some(data)) => {
                if data.len() > MAX_BULK_LEN {
                    return Err(EncodeError::TooLarge);
                }
                out.push(b'$');
                out.extend_from_slice(data.len().to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                out.extend_from_slice(data);
                out.extend_from_slice(b"\r\n");
            },
            Self::Array(None) => out.extend_from_slice(b"*-1\r\n"),
            Self::Array(Some(items)) => {
                if items.len() > MAX_ARRAY_LEN {
                    return Err(EncodeError::TooLarge);
                }
                out.push(b'*');
                out.extend_from_slice(items.len().to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                for item in items {
                    item.encode_value(out, depth + 1)?;
                }
            },
        }

        Ok(())
    }

    fn encode_line(
        out: &mut Vec<u8>,
        prefix: u8,
        s: &str,
    ) -> Result<(), EncodeError> {
        if s.bytes().any(|b| b == b'\r' || b == b'\n') {
            return Err(EncodeError::InvalidString);
        }
        out.push(prefix);
        out.extend_from_slice(s.as_bytes());
        out.extend_from_slice(b"\r\n");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- value constructors ----------

    fn simple(s: &str) -> RespValue {
        RespValue::SimpleString(s.into())
    }

    fn error(s: &str) -> RespValue {
        RespValue::Error(s.into())
    }

    fn int(n: i64) -> RespValue {
        RespValue::Integer(n)
    }

    fn bulk(data: impl AsRef<[u8]>) -> RespValue {
        RespValue::BulkString(Some(Bytes::copy_from_slice(data.as_ref())))
    }

    fn null_bulk() -> RespValue {
        RespValue::BulkString(None)
    }

    fn array(items: Vec<RespValue>) -> RespValue {
        RespValue::Array(Some(items))
    }

    fn null_array() -> RespValue {
        RespValue::Array(None)
    }

    // ---------- parse helpers ----------

    fn parse(input: &[u8]) -> Result<(RespValue, usize), ParseError> {
        RespParser::new(input).parse()
    }

    /// Parses `input`, asserting it is exactly one complete frame.
    #[track_caller]
    fn ok(input: &[u8]) -> RespValue {
        let (value, consumed) =
            parse(input).expect("expected successful parse");
        assert_eq!(consumed, input.len(), "should consume entire input");
        value
    }

    #[track_caller]
    fn assert_err(input: &[u8], expected: ParseError) {
        assert_eq!(
            parse(input),
            Err(expected),
            "input: {:?}",
            String::from_utf8_lossy(input)
        );
    }

    #[track_caller]
    fn assert_all_err(inputs: &[&[u8]], expected: ParseError) {
        for input in inputs {
            assert_err(input, expected);
        }
    }

    /// Asserts that `frame` parses, and that every strict prefix of it is
    /// reported as `Incomplete` (never an error, never a bogus success).
    #[track_caller]
    fn assert_prefixes_incomplete(frame: &[u8]) {
        for len in 0..frame.len() {
            assert_eq!(
                parse(&frame[..len]),
                Err(ParseError::Incomplete),
                "prefix length {len} of {:?}",
                String::from_utf8_lossy(frame)
            );
        }

        assert!(parse(frame).is_ok(), "full frame should parse");
    }

    /// `levels` nested single-element arrays; the innermost is an empty array.
    fn nested_frame(levels: usize) -> Vec<u8> {
        let mut buf = b"*1\r\n".repeat(levels);
        buf.extend_from_slice(b"*0\r\n");
        buf
    }

    /// The same shape as `nested_frame`, as a value (for the encoder).
    fn nested_value(levels: usize) -> RespValue {
        (0..levels).fold(array(vec![]), |inner, _| array(vec![inner]))
    }

    // ======================================================================
    // Parsing
    // ======================================================================

    mod simple_and_error {
        use super::*;

        #[test]
        fn simple_string() {
            assert_eq!(ok(b"+OK\r\n"), simple("OK"));
            assert_eq!(ok(b"+\r\n"), simple(""));
        }

        #[test]
        fn simple_string_with_spaces_and_unicode() {
            assert_eq!(
                ok("+héllo wörld\r\n".as_bytes()),
                simple("héllo wörld")
            );
        }

        #[test]
        fn simple_string_embedded_cr_preserved() {
            assert_eq!(ok(b"+a\rb\r\n"), simple("a\rb"));
        }

        #[test]
        fn bare_cr_or_lf_is_not_a_terminator() {
            assert_all_err(&[b"+OK\n", b"+OK\r"], ParseError::Incomplete);
        }

        #[test]
        fn error_value() {
            assert_eq!(
                ok(b"-ERR unknown command\r\n"),
                error("ERR unknown command")
            );
        }

        #[test]
        fn invalid_utf8_is_rejected() {
            assert_all_err(
                &[b"+\xff\xfe\r\n", b"-\xc3\x28\r\n"],
                ParseError::InvalidUtf8,
            );
        }
    }

    mod integer {
        use super::*;

        #[test]
        fn signs_and_zero() {
            let cases: [(&[u8], i64); 4] = [
                (b":1000\r\n", 1000),
                (b":-42\r\n", -42),
                (b":0\r\n", 0),
                (b":+7\r\n", 7),
            ];

            for (input, expected) in cases {
                assert_eq!(ok(input), int(expected));
            }
        }

        #[test]
        fn bounds() {
            assert_eq!(ok(b":9223372036854775807\r\n"), int(i64::MAX));
            assert_eq!(ok(b":-9223372036854775808\r\n"), int(i64::MIN));
        }

        #[test]
        fn invalid() {
            assert_all_err(
                &[
                    b":\r\n",
                    b":abc\r\n",
                    b":12a\r\n",
                    b":1 2\r\n",
                    b": 1\r\n",
                    b":1.5\r\n",
                    b":\xff\r\n",
                    // One past i64::MAX / one below i64::MIN.
                    b":9223372036854775808\r\n",
                    b":-9223372036854775809\r\n",
                ],
                ParseError::InvalidInteger,
            );
        }
    }

    mod bulk {
        use super::*;

        #[test]
        fn basic_empty_and_null() {
            assert_eq!(ok(b"$5\r\nhello\r\n"), bulk("hello"));
            assert_eq!(ok(b"$0\r\n\r\n"), bulk(""));
            assert_eq!(ok(b"$-1\r\n"), null_bulk());
        }

        #[test]
        fn empty_is_not_null() {
            assert_ne!(ok(b"$0\r\n\r\n"), ok(b"$-1\r\n"));
        }

        #[test]
        fn binary_safe() {
            assert_eq!(
                ok(b"$6\r\n\x00\xff\r\n\x01\x02\r\n"),
                bulk([0x00, 0xff, b'\r', b'\n', 0x01, 0x02])
            );
        }

        #[test]
        fn payload_containing_crlf() {
            assert_eq!(ok(b"$4\r\na\r\nb\r\n"), bulk("a\r\nb"));
        }

        #[test]
        fn consumed_count_with_trailing_data() {
            let (value, consumed) = parse(b"$3\r\nfoo\r\n+extra\r\n").unwrap();
            assert_eq!(value, bulk("foo"));
            assert_eq!(consumed, 9);
        }

        #[test]
        fn missing_or_misplaced_crlf() {
            // Wrong terminator bytes.
            assert_err(b"$3\r\nfooXY", ParseError::MissingCrlf);
            // Declared 3 but 5 bytes of payload precede the CRLF.
            assert_err(b"$3\r\nhello\r\n", ParseError::MissingCrlf);
        }

        #[test]
        fn truncated_payload_is_incomplete() {
            assert_all_err(
                &[b"$5\r\nhel", b"$5\r\nhello", b"$5\r\nhello\r"],
                ParseError::Incomplete,
            );
        }

        #[test]
        fn invalid_length() {
            assert_all_err(
                &[
                    b"$abc\r\n",
                    b"$\r\n",
                    b"$-2\r\n",
                    b"$-100\r\n",
                    b"$1.5\r\n",
                    b"$\xff\r\n",
                    // Larger than i64, so it fails integer parsing.
                    b"$99999999999999999999\r\n",
                ],
                ParseError::InvalidLength,
            );
        }

        #[test]
        fn too_large() {
            let over = format!("${}\r\n", MAX_BULK_LEN + 1);
            assert_err(over.as_bytes(), ParseError::TooLarge);

            // Fits in i64 but is absurd; must not panic or try to allocate.
            assert_err(b"$9223372036854775807\r\n", ParseError::TooLarge);
        }

        #[test]
        fn at_limit_is_incomplete_not_too_large() {
            let at = format!("${MAX_BULK_LEN}\r\n");
            assert_err(at.as_bytes(), ParseError::Incomplete);
        }
    }

    mod array {
        use super::*;

        #[test]
        fn empty_and_null() {
            assert_eq!(ok(b"*0\r\n"), array(vec![]));
            assert_eq!(ok(b"*-1\r\n"), null_array());
        }

        #[test]
        fn of_bulk_strings() {
            assert_eq!(
                ok(b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n"),
                array(vec![bulk("foo"), bulk("bar")])
            );
        }

        #[test]
        fn typical_redis_command() {
            assert_eq!(
                ok(b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"),
                array(vec![bulk("SET"), bulk("key"), bulk("value")])
            );
        }

        #[test]
        fn mixed_types() {
            assert_eq!(
                ok(b"*5\r\n:1\r\n+two\r\n-three\r\n$4\r\nfour\r\n$-1\r\n"),
                array(vec![
                    int(1),
                    simple("two"),
                    error("three"),
                    bulk("four"),
                    null_bulk(),
                ])
            );
        }

        #[test]
        fn nested() {
            assert_eq!(
                ok(b"*2\r\n*2\r\n:1\r\n:2\r\n*1\r\n*0\r\n"),
                array(vec![
                    array(vec![int(1), int(2)]),
                    array(vec![array(vec![])]),
                ])
            );
        }

        #[test]
        fn containing_null_array() {
            assert_eq!(ok(b"*1\r\n*-1\r\n"), array(vec![null_array()]));
        }

        #[test]
        fn invalid_length() {
            assert_all_err(
                &[b"*abc\r\n", b"*\r\n", b"*-2\r\n", b"*1.0\r\n"],
                ParseError::InvalidLength,
            );
        }

        #[test]
        fn too_large() {
            let over = format!("*{}\r\n", MAX_ARRAY_LEN + 1);
            assert_err(over.as_bytes(), ParseError::TooLarge);
        }

        #[test]
        fn fewer_elements_than_declared_is_incomplete() {
            assert_err(b"*3\r\n:1\r\n:2\r\n", ParseError::Incomplete);
        }

        #[test]
        fn max_length_header_without_body_is_incomplete() {
            // Declares the largest allowed array but sends no elements. This must
            // report `Incomplete` without pre-allocating for a million elements.
            let header = format!("*{MAX_ARRAY_LEN}\r\n");
            assert_err(header.as_bytes(), ParseError::Incomplete);
        }

        #[test]
        fn element_error_propagates() {
            assert_err(b"*2\r\n:1\r\n?bad\r\n", ParseError::InvalidType(b'?'));
            assert_err(b"*2\r\n:1\r\n:xyz\r\n", ParseError::InvalidInteger);
        }

        #[test]
        fn trailing_data_not_consumed() {
            let (value, consumed) = parse(b"*1\r\n:1\r\n:2\r\n").unwrap();
            assert_eq!(value, array(vec![int(1)]));
            assert_eq!(consumed, 8);
        }
    }

    mod depth {
        use super::*;

        #[test]
        fn at_limit_is_ok() {
            // The outer array is depth 0; the innermost sits at MAX_DEPTH.
            assert!(parse(&nested_frame(MAX_DEPTH)).is_ok());
        }

        #[test]
        fn over_limit_is_rejected() {
            assert_err(&nested_frame(MAX_DEPTH + 1), ParseError::TooDeep);
        }

        #[test]
        fn very_deep_nesting_does_not_overflow_the_stack() {
            assert_err(&nested_frame(100_000), ParseError::TooDeep);
        }
    }

    mod framing {
        use super::*;

        #[test]
        fn empty_input_is_incomplete() {
            assert_err(b"", ParseError::Incomplete);
        }

        #[test]
        fn type_byte_only_is_incomplete() {
            assert_all_err(
                &[b"+", b"-", b":", b"$", b"*"],
                ParseError::Incomplete,
            );
        }

        #[test]
        fn invalid_type_byte() {
            assert_err(b"?foo\r\n", ParseError::InvalidType(b'?'));
            assert_err(b"\r\n", ParseError::InvalidType(b'\r'));
            assert_err(b"\x00", ParseError::InvalidType(0));
        }

        #[test]
        fn every_strict_prefix_of_a_frame_is_incomplete() {
            let frames: &[&[u8]] = &[
                b"+OK\r\n",
                b"-ERR x\r\n",
                b":42\r\n",
                b"$5\r\nhello\r\n",
                b"$-1\r\n",
                b"*3\r\n$3\r\nSET\r\n:42\r\n+OK\r\n",
                b"*2\r\n*1\r\n:1\r\n*0\r\n",
            ];

            for frame in frames {
                assert_prefixes_incomplete(frame);
            }
        }

        #[test]
        fn pipelined_frames_parse_sequentially() {
            let input = b"+OK\r\n:5\r\n$2\r\nhi\r\n";
            let mut offset = 0;
            let mut parsed = Vec::new();

            while offset < input.len() {
                let (value, used) = parse(&input[offset..]).unwrap();
                parsed.push(value);
                offset += used;
            }

            assert_eq!(parsed, vec![simple("OK"), int(5), bulk("hi")]);
        }

        #[test]
        fn parse_error_display_messages() {
            assert_eq!(ParseError::Incomplete.to_string(), "incomplete frame");
            assert_eq!(
                ParseError::InvalidType(b'?').to_string(),
                "invalid type byte: 63"
            );
            assert_eq!(
                ParseError::TooDeep.to_string(),
                "frame nested too deeply"
            );
        }
    }

    // ======================================================================
    // Encoding
    // ======================================================================

    mod encode {
        use super::*;

        #[track_caller]
        fn assert_encodes(value: &RespValue, expected: &[u8]) {
            assert_eq!(value.encode().unwrap(), expected, "{value:?}");
        }

        #[test]
        fn each_type() {
            let cases: Vec<(RespValue, &[u8])> = vec![
                (simple("OK"), b"+OK\r\n"),
                (simple(""), b"+\r\n"),
                (error("ERR bad"), b"-ERR bad\r\n"),
                (int(0), b":0\r\n"),
                (int(-42), b":-42\r\n"),
                (int(i64::MAX), b":9223372036854775807\r\n"),
                (int(i64::MIN), b":-9223372036854775808\r\n"),
                (bulk("hello"), b"$5\r\nhello\r\n"),
                (bulk(""), b"$0\r\n\r\n"),
                (null_bulk(), b"$-1\r\n"),
                (null_array(), b"*-1\r\n"),
                (array(vec![]), b"*0\r\n"),
            ];

            for (value, expected) in &cases {
                assert_encodes(value, expected);
            }
        }

        #[test]
        fn nested_array() {
            let value =
                array(vec![bulk("SET"), array(vec![int(1)]), null_bulk()]);

            assert_encodes(&value, b"*3\r\n$3\r\nSET\r\n*1\r\n:1\r\n$-1\r\n");
        }

        #[test]
        fn bulk_string_is_binary_safe() {
            assert_encodes(
                &bulk([0, b'\r', b'\n', 0xff]),
                b"$4\r\n\x00\r\n\xff\r\n",
            );
        }

        #[test]
        fn rejects_crlf_in_simple_string_and_error() {
            for s in ["a\r\nb", "a\rb", "a\nb"] {
                assert_eq!(simple(s).encode(), Err(EncodeError::InvalidString));
                assert_eq!(error(s).encode(), Err(EncodeError::InvalidString));
            }
        }

        #[test]
        fn failed_encode_leaves_buffer_unchanged() {
            let mut out = b"prefix".to_vec();
            let value = array(vec![int(1), simple("bad\r\n")]);

            assert_eq!(
                value.encode_into(&mut out),
                Err(EncodeError::InvalidString)
            );
            assert_eq!(out, b"prefix");
        }

        #[test]
        fn encode_into_appends() {
            let mut out = Vec::new();
            simple("OK").encode_into(&mut out).unwrap();
            int(5).encode_into(&mut out).unwrap();

            assert_eq!(out, b"+OK\r\n:5\r\n");
        }

        #[test]
        fn depth_limit_matches_parser() {
            assert!(nested_value(MAX_DEPTH).encode().is_ok());
            assert_eq!(
                nested_value(MAX_DEPTH + 1).encode(),
                Err(EncodeError::TooDeep)
            );
        }

        #[test]
        fn array_length_limit() {
            let value = array(vec![int(0); MAX_ARRAY_LEN + 1]);

            assert_eq!(value.encode(), Err(EncodeError::TooLarge));
        }

        #[test]
        fn encode_error_display_messages() {
            assert_eq!(
                EncodeError::InvalidString.to_string(),
                "simple string/error contains CR or LF"
            );
            assert_eq!(
                EncodeError::TooDeep.to_string(),
                "value nested too deeply"
            );
        }

        #[test]
        fn round_trip() {
            let values = vec![
                simple("héllo"),
                error("ERR x"),
                int(i64::MIN),
                null_bulk(),
                bulk("a\r\nb"),
                bulk((0..=255u8).collect::<Vec<_>>()),
                null_array(),
                array(vec![
                    array(vec![int(1), bulk("x")]),
                    null_array(),
                    array(vec![]),
                ]),
            ];

            for value in values {
                let bytes = value.encode().unwrap();
                let (parsed, consumed) = parse(&bytes).unwrap();

                assert_eq!(parsed, value);
                assert_eq!(consumed, bytes.len());
            }
        }
    }
}
