//! Field level encoding of the TWS API text protocol.
//!
//! A frame is a 4-byte big-endian length followed by the payload. The payload
//! is a sequence of fields, each terminated by a NUL byte.

use crate::consts::{INFINITY_STR, MAX_MSG_LEN, UNSET_DOUBLE, UNSET_INTEGER};
use crate::types::TagValue;
use std::borrow::Cow;

/// Errors produced while decoding a payload.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("message ended early: expected another field after field {0}")]
    MissingField(usize),
    #[error("field {index} is {value:?}, but a number was expected")]
    BadNumber { index: usize, value: String },
    #[error("frame of {0} bytes is larger than the limit of {MAX_MSG_LEN} bytes")]
    FrameTooLarge(usize),
    #[error("protobuf message is malformed: {0}")]
    Protobuf(String),
}

/// Appends fields to a reusable buffer and produces length-prefixed frames.
#[derive(Debug, Default)]
pub struct FieldWriter {
    buf: Vec<u8>,
}

impl FieldWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(512),
        }
    }

    /// Clear the buffer and reserve the 4-byte length prefix.
    #[inline]
    pub fn begin(&mut self) {
        self.buf.clear();
        self.buf.extend_from_slice(&[0, 0, 0, 0]);
    }

    /// Append a string field.
    #[inline]
    pub fn str(&mut self, v: &str) {
        self.buf.extend_from_slice(v.as_bytes());
        self.buf.push(0);
    }

    /// Append an integer field.
    #[inline]
    pub fn int(&mut self, v: impl itoa::Integer) {
        let mut b = itoa::Buffer::new();
        self.buf.extend_from_slice(b.format(v).as_bytes());
        self.buf.push(0);
    }

    /// Append an integer field, empty when the value is `UNSET_INTEGER`.
    #[inline]
    pub fn int_max(&mut self, v: i32) {
        if v == UNSET_INTEGER {
            self.buf.push(0);
        } else {
            self.int(v);
        }
    }

    /// Append a float field with its literal value (IB's `make_field`).
    #[inline]
    pub fn f64(&mut self, v: f64) {
        write_f64(&mut self.buf, v);
        self.buf.push(0);
    }

    /// Append a float field, empty when unset (IB's `make_field_handle_empty`).
    #[inline]
    pub fn f64_max(&mut self, v: f64) {
        if v != UNSET_DOUBLE {
            write_f64(&mut self.buf, v);
        }
        self.buf.push(0);
    }

    /// Append a boolean as `1` or `0`.
    #[inline]
    pub fn bool(&mut self, v: bool) {
        self.buf.push(if v { b'1' } else { b'0' });
        self.buf.push(0);
    }

    /// Append a tag/value list as one field `tag=value;tag=value;`.
    pub fn tag_values(&mut self, v: &[TagValue]) {
        for tv in v {
            self.buf.extend_from_slice(tv.tag.as_bytes());
            self.buf.push(b'=');
            self.buf.extend_from_slice(tv.value.as_bytes());
            self.buf.push(b';');
        }
        self.buf.push(0);
    }

    /// Patch the length prefix and return the complete frame.
    #[inline]
    pub fn finish(&mut self) -> &[u8] {
        let len = (self.buf.len() - 4) as u32;
        self.buf[..4].copy_from_slice(&len.to_be_bytes());
        &self.buf
    }

    /// Take the frame bytes out of the writer.
    pub fn take(&mut self) -> Vec<u8> {
        self.finish();
        std::mem::take(&mut self.buf)
    }
}

/// Write a float in the shortest form TWS accepts.
#[inline]
pub fn write_f64(buf: &mut Vec<u8>, v: f64) {
    if v.is_finite() {
        if v == v.trunc() && v.abs() < 1e15 {
            let mut b = itoa::Buffer::new();
            buf.extend_from_slice(b.format(v as i64).as_bytes());
        } else {
            let mut b = ryu::Buffer::new();
            buf.extend_from_slice(b.format_finite(v).as_bytes());
        }
    } else if v == f64::INFINITY {
        buf.extend_from_slice(INFINITY_STR.as_bytes());
    } else if v == f64::NEG_INFINITY {
        buf.extend_from_slice(b"-Infinite");
    }
}

/// Reads fields sequentially from one payload without allocating.
#[derive(Debug, Clone)]
pub struct FieldReader<'a> {
    rest: &'a [u8],
    index: usize,
}

impl<'a> FieldReader<'a> {
    pub fn new(payload: &'a [u8]) -> Self {
        Self {
            rest: payload,
            index: 0,
        }
    }

    /// Number of fields consumed so far.
    pub fn index(&self) -> usize {
        self.index
    }

    /// True when no complete field remains.
    pub fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    /// Raw bytes of the next field.
    #[inline]
    pub fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        match memchr::memchr(0, self.rest) {
            Some(pos) => {
                let field = &self.rest[..pos];
                self.rest = &self.rest[pos + 1..];
                self.index += 1;
                Ok(field)
            }
            None => Err(DecodeError::MissingField(self.index)),
        }
    }

    /// Next field as text; invalid UTF-8 is replaced.
    #[inline]
    pub fn text(&mut self) -> Result<Cow<'a, str>, DecodeError> {
        Ok(String::from_utf8_lossy(self.bytes()?))
    }

    /// Next field as an owned string.
    #[inline]
    pub fn string(&mut self) -> Result<String, DecodeError> {
        Ok(self.text()?.into_owned())
    }

    /// Next field as a string, with `\uXXXX` style escapes resolved (IB sends
    /// non-ASCII text escaped at server version 153 and later).
    pub fn string_unescaped(&mut self) -> Result<String, DecodeError> {
        let s = self.text()?;
        Ok(if s.contains('\\') {
            unescape(&s)
        } else {
            s.into_owned()
        })
    }

    /// Skip one field.
    #[inline]
    pub fn skip(&mut self) -> Result<(), DecodeError> {
        self.bytes().map(|_| ())
    }

    fn bad(&self, field: &[u8]) -> DecodeError {
        DecodeError::BadNumber {
            index: self.index.saturating_sub(1),
            value: String::from_utf8_lossy(field).into_owned(),
        }
    }

    #[inline]
    fn long_or(&mut self, empty: i64) -> Result<i64, DecodeError> {
        let f = self.bytes()?;
        if f.is_empty() {
            return Ok(empty);
        }
        // A decimal value in an integer field is truncated.
        parse_i64(f)
            .or_else(|| {
                fast_float2::parse::<f64, _>(f)
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(|v| v as i64)
            })
            .ok_or_else(|| self.bad(f))
    }

    /// Next field as `i64`; empty is 0.
    #[inline]
    pub fn long(&mut self) -> Result<i64, DecodeError> {
        self.long_or(0)
    }

    /// Next field as `i32`; empty is 0.
    #[inline]
    pub fn int(&mut self) -> Result<i32, DecodeError> {
        self.long_or(0).map(clamp_i32)
    }

    /// Next field as `i32`; empty is `UNSET_INTEGER`.
    #[inline]
    pub fn int_max(&mut self) -> Result<i32, DecodeError> {
        self.long_or(UNSET_INTEGER as i64).map(clamp_i32)
    }

    #[inline]
    fn parse_float(&self, f: &[u8]) -> Result<f64, DecodeError> {
        match fast_float2::parse::<f64, _>(f) {
            Ok(v) => Ok(v),
            Err(_) => match f {
                b"Infinite" | b"Infinity" | b"inf" => Ok(f64::INFINITY),
                b"-Infinite" | b"-Infinity" | b"-inf" => Ok(f64::NEG_INFINITY),
                _ => Err(self.bad(f)),
            },
        }
    }

    #[inline]
    fn float_or(&mut self, empty: f64) -> Result<f64, DecodeError> {
        let f = self.bytes()?;
        if f.is_empty() {
            Ok(empty)
        } else {
            self.parse_float(f)
        }
    }

    /// Next field as `f64`; empty is 0.
    #[inline]
    pub fn f64(&mut self) -> Result<f64, DecodeError> {
        self.float_or(0.0)
    }

    /// Next field as `f64`; empty is `UNSET_DOUBLE`.
    #[inline]
    pub fn f64_max(&mut self) -> Result<f64, DecodeError> {
        self.float_or(UNSET_DOUBLE)
    }

    /// Next field as a decimal quantity (IB's `Decimal`); empty and the IB
    /// "unset" spellings are `UNSET_DOUBLE`.
    #[inline]
    pub fn decimal(&mut self) -> Result<f64, DecodeError> {
        match self.bytes()? {
            b""
            | b"2147483647"
            | b"9223372036854775807"
            | b"-9223372036854775808"
            | b"1.7976931348623157E308" => Ok(UNSET_DOUBLE),
            f => self.parse_float(f),
        }
    }

    /// Next field as a boolean (`1`, `0`, `true`, `false`; empty is false).
    #[inline]
    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        let f = self.bytes()?;
        Ok(match f {
            b"" | b"0" | b"false" | b"False" => false,
            b"1" | b"true" | b"True" => true,
            _ => parse_i64(f).map(|v| v != 0).ok_or_else(|| self.bad(f))?,
        })
    }
}

fn clamp_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Parse an ASCII decimal integer with optional sign.
#[inline]
pub fn parse_i64(f: &[u8]) -> Option<i64> {
    let (neg, digits) = match f.first()? {
        b'-' => (true, &f[1..]),
        b'+' => (false, &f[1..]),
        _ => (false, f),
    };
    if digits.is_empty() || digits.len() > 19 {
        return None;
    }
    let mut v: i64 = 0;
    for &c in digits {
        let d = c.wrapping_sub(b'0');
        if d > 9 {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(d as i64)?;
    }
    Some(if neg { -v } else { v })
}

/// Resolve Python `unicode-escape` style sequences (`\uXXXX`, `\xNN`, `\n`, ...).
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some(k @ ('u' | 'x' | 'U')) => {
                let n = match k {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let hex: String = (0..n).filter_map(|_| chars.next()).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) if hex.len() == n => out.push(ch),
                    _ => {
                        out.push('\\');
                        out.push(k);
                        out.push_str(&hex);
                    }
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Accumulates socket bytes and yields complete frame payloads.
#[derive(Debug, Default)]
pub struct FrameBuffer {
    buf: Vec<u8>,
    pos: usize,
}

impl FrameBuffer {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(64 * 1024),
            pos: 0,
        }
    }

    /// Spare capacity to read into; call [`FrameBuffer::commit`] afterwards.
    pub fn read_buf(&mut self, min: usize) -> &mut Vec<u8> {
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > self.buf.capacity() / 2 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.reserve(min);
        &mut self.buf
    }

    /// Append bytes received from the socket.
    pub fn extend(&mut self, data: &[u8]) {
        self.read_buf(data.len()).extend_from_slice(data);
    }

    /// Next complete payload, if any.
    #[inline]
    pub fn next_frame(&mut self) -> Result<Option<&[u8]>, DecodeError> {
        let avail = &self.buf[self.pos..];
        if avail.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([avail[0], avail[1], avail[2], avail[3]]) as usize;
        if len > MAX_MSG_LEN {
            return Err(DecodeError::FrameTooLarge(len));
        }
        if avail.len() < 4 + len {
            return Ok(None);
        }
        let start = self.pos + 4;
        self.pos = start + len;
        Ok(Some(&self.buf[start..start + len]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_are_written_in_short_form() {
        let mut b = Vec::new();
        for (v, s) in [
            (100.0, "100"),
            (0.5, "0.5"),
            (-2.25, "-2.25"),
            (f64::INFINITY, "Infinite"),
            (UNSET_DOUBLE, "1.7976931348623157e308"),
            (1e20, "1e20"),
        ] {
            b.clear();
            write_f64(&mut b, v);
            assert_eq!(std::str::from_utf8(&b).unwrap(), s);
        }
    }

    #[test]
    fn writer_patches_length_prefix() {
        let mut w = FieldWriter::new();
        w.begin();
        w.int(71);
        w.int(2);
        w.int(1);
        w.str("");
        assert_eq!(w.finish(), b"\0\0\0\x0871\x002\x001\x00\x00");
    }

    #[test]
    fn reader_parses_sentinels_and_numbers() {
        let mut r = FieldReader::new(b"12\0\0-3.5\0\x001\0Infinite\0abc\0");
        assert_eq!(r.int().unwrap(), 12);
        assert_eq!(r.int_max().unwrap(), UNSET_INTEGER);
        assert_eq!(r.f64().unwrap(), -3.5);
        assert_eq!(r.f64_max().unwrap(), UNSET_DOUBLE);
        assert!(r.bool().unwrap());
        assert_eq!(r.f64().unwrap(), f64::INFINITY);
        assert!(matches!(
            r.int(),
            Err(DecodeError::BadNumber { index: 6, .. })
        ));
        assert_eq!(r.bytes(), Err(DecodeError::MissingField(7)));
    }

    #[test]
    fn unescape_resolves_unicode() {
        assert_eq!(unescape(r"Café \x41\\"), "Café A\\");
    }

    #[test]
    fn frame_buffer_handles_partial_frames() {
        let mut fb = FrameBuffer::new();
        fb.extend(b"\0\0\0\x049\0");
        assert_eq!(fb.next_frame().unwrap(), None);
        fb.extend(b"1\0\0\0\0\x0315\0");
        assert_eq!(fb.next_frame().unwrap(), Some(&b"9\x001\x00"[..]));
        assert_eq!(fb.next_frame().unwrap(), Some(&b"15\x00"[..]));
        assert_eq!(fb.next_frame().unwrap(), None);
    }
}
