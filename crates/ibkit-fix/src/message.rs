//! FIX message encoding and zero-copy parsing.

use crate::tags;

pub const SOH: u8 = 0x01;

/// Errors found while parsing a FIX message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("message must start with 8=BeginString, then 9=BodyLength, then 35=MsgType")]
    BadHeader,
    #[error("field {0:?} is not tag=value")]
    BadField(String),
    #[error("BodyLength says {declared} bytes but the body has {actual} bytes")]
    BodyLength { declared: usize, actual: usize },
    #[error("CheckSum is {declared:03} but the bytes sum to {actual:03}")]
    CheckSum { declared: u32, actual: u32 },
}

/// A parsed message that borrows the input buffer.
#[derive(Debug, Clone)]
pub struct MessageView<'a> {
    pub raw: &'a [u8],
    fields: Vec<(u32, &'a [u8])>,
}

impl<'a> MessageView<'a> {
    /// Parse and validate one complete message.
    pub fn parse(raw: &'a [u8]) -> Result<Self, ParseError> {
        let mut fields = Vec::with_capacity(32);
        for f in raw.split(|b| *b == SOH) {
            if f.is_empty() {
                continue;
            }
            let eq = memchr::memchr(b'=', f)
                .ok_or_else(|| ParseError::BadField(String::from_utf8_lossy(f).into()))?;
            let tag = ibkit_tws::wire::parse_i64(&f[..eq])
                .filter(|t| *t > 0)
                .ok_or_else(|| ParseError::BadField(String::from_utf8_lossy(f).into()))?;
            fields.push((tag as u32, &f[eq + 1..]));
        }
        if fields.len() < 4
            || fields[0].0 != tags::BEGIN_STRING
            || fields[1].0 != tags::BODY_LENGTH
            || fields[2].0 != tags::MSG_TYPE
        {
            return Err(ParseError::BadHeader);
        }
        let last = fields.last().expect("checked length");
        if last.0 != tags::CHECK_SUM {
            return Err(ParseError::BadHeader);
        }
        // BodyLength counts from after "9=..|" to before "10=".
        let body_start = 2 + fields[0].1.len() + 1 + 2 + fields[1].1.len() + 1;
        let trailer_start = raw.len() - (3 + last.1.len() + 1);
        let declared: usize = std::str::from_utf8(fields[1].1)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(ParseError::BadHeader)?;
        let actual = trailer_start.saturating_sub(body_start);
        if declared != actual {
            return Err(ParseError::BodyLength { declared, actual });
        }
        let sum = checksum(&raw[..trailer_start]);
        let declared_sum: u32 = std::str::from_utf8(last.1)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(ParseError::BadHeader)?;
        if sum != declared_sum {
            return Err(ParseError::CheckSum {
                declared: declared_sum,
                actual: sum,
            });
        }
        Ok(Self { raw, fields })
    }

    pub fn msg_type(&self) -> &'a str {
        std::str::from_utf8(self.fields[2].1).unwrap_or("")
    }

    /// First value of `tag`.
    pub fn get(&self, tag: u32) -> Option<&'a str> {
        self.fields
            .iter()
            .find(|(t, _)| *t == tag)
            .and_then(|(_, v)| std::str::from_utf8(v).ok())
    }

    pub fn get_u64(&self, tag: u32) -> Option<u64> {
        self.get(tag)?.parse().ok()
    }

    pub fn seq_num(&self) -> u64 {
        self.get_u64(tags::MSG_SEQ_NUM).unwrap_or(0)
    }

    pub fn is_poss_dup(&self) -> bool {
        self.get(tags::POSS_DUP_FLAG) == Some("Y")
    }

    /// All fields in order.
    pub fn fields(&self) -> impl Iterator<Item = (u32, &'a str)> + '_ {
        self.fields
            .iter()
            .map(|(t, v)| (*t, std::str::from_utf8(v).unwrap_or("")))
    }

    /// Owned copy of the body fields (everything except 8, 9, 10).
    pub fn to_message(&self) -> Message {
        let mut m = Message::new(self.msg_type());
        for (t, v) in self.fields() {
            if !matches!(
                t,
                tags::BEGIN_STRING | tags::BODY_LENGTH | tags::MSG_TYPE | tags::CHECK_SUM
            ) {
                m.fields.push((t, v.to_string()));
            }
        }
        m
    }
}

/// Sum of bytes modulo 256.
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |a, b| a.wrapping_add(*b as u32)) % 256
}

/// An owned message: type plus ordered fields (header fields other than 8,
/// 9 and 35 included, if set).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Message {
    pub msg_type: String,
    pub fields: Vec<(u32, String)>,
}

impl Message {
    pub fn new(msg_type: &str) -> Self {
        Self {
            msg_type: msg_type.into(),
            fields: Vec::new(),
        }
    }

    /// Append a field (repeating groups are appended in order).
    pub fn push(&mut self, tag: u32, value: impl ToString) -> &mut Self {
        self.fields.push((tag, value.to_string()));
        self
    }

    /// Append a float in shortest round-trip form.
    pub fn push_f64(&mut self, tag: u32, value: f64) -> &mut Self {
        let s = if value == value.trunc() && value.abs() < 1e15 {
            itoa::Buffer::new().format(value as i64).to_string()
        } else {
            ryu::Buffer::new().format(value).to_string()
        };
        self.fields.push((tag, s));
        self
    }

    /// Replace the first value of `tag`, or append it.
    pub fn set(&mut self, tag: u32, value: impl ToString) -> &mut Self {
        match self.fields.iter_mut().find(|(t, _)| *t == tag) {
            Some(f) => f.1 = value.to_string(),
            None => self.fields.push((tag, value.to_string())),
        }
        self
    }

    pub fn get(&self, tag: u32) -> Option<&str> {
        self.fields
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v.as_str())
    }

    pub fn remove(&mut self, tag: u32) {
        self.fields.retain(|(t, _)| *t != tag);
    }

    /// Encode with `begin_string` and the header fields given first.
    pub fn encode(&self, begin_string: &str, header: &[(u32, &str)], out: &mut Vec<u8>) {
        let mut body: Vec<u8> = Vec::with_capacity(256);
        let put = |b: &mut Vec<u8>, t: u32, v: &str| {
            b.extend_from_slice(itoa::Buffer::new().format(t).as_bytes());
            b.push(b'=');
            b.extend_from_slice(v.as_bytes());
            b.push(SOH);
        };
        put(&mut body, tags::MSG_TYPE, &self.msg_type);
        for (t, v) in header {
            put(&mut body, *t, v);
        }
        for (t, v) in &self.fields {
            if !header.iter().any(|(h, _)| h == t) {
                put(&mut body, *t, v);
            }
        }
        let start = out.len();
        put(out, tags::BEGIN_STRING, begin_string);
        put(
            out,
            tags::BODY_LENGTH,
            itoa::Buffer::new().format(body.len()),
        );
        out.extend_from_slice(&body);
        let sum = checksum(&out[start..]);
        put(out, tags::CHECK_SUM, &format!("{sum:03}"));
    }
}

/// FIXCOMP block: `8=FIXCOMP|9=<length>|` followed by `length` bytes of zlib
/// data holding one or more complete FIX messages (IB's message compression).
pub fn fixcomp(messages: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(messages).expect("writing to a Vec cannot fail");
    let data = z.finish().expect("writing to a Vec cannot fail");
    let mut out = format!("8=FIXCOMP\x019={}\x01", data.len()).into_bytes();
    out.extend(data);
    out
}

/// Inflate a FIXCOMP payload (zlib, or raw deflate as a fallback).
fn inflate(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .is_ok()
    {
        return Ok(out);
    }
    out.clear();
    flate2::read::DeflateDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

/// Splits a byte stream into complete FIX messages; FIXCOMP blocks are
/// inflated into the messages they hold. Consumed bytes are skipped with an
/// offset and dropped at the next `extend`, so a read that holds many
/// messages is split in linear time.
#[derive(Debug, Default)]
pub struct FrameSplitter {
    buf: Vec<u8>,
    /// Start of the unconsumed bytes in `buf`.
    start: usize,
    inflated: std::collections::VecDeque<Vec<u8>>,
}

impl FrameSplitter {
    pub fn extend(&mut self, data: &[u8]) {
        self.buf.drain(..self.start);
        self.start = 0;
        self.buf.extend_from_slice(data);
    }

    /// Next complete raw message, if the buffer holds one.
    pub fn next_message(&mut self) -> Option<Vec<u8>> {
        if let Some(m) = self.inflated.pop_front() {
            return Some(m);
        }
        const COMP: &[u8] = b"8=FIXCOMP\x019=";
        loop {
            let rest = &self.buf[self.start..];
            if rest.starts_with(COMP) {
                let len_end = memchr::memchr(SOH, &rest[COMP.len()..])? + COMP.len();
                let len: usize = std::str::from_utf8(&rest[COMP.len()..len_end])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let end = len_end + 1 + len;
                if rest.len() < end {
                    return None;
                }
                match inflate(&rest[len_end + 1..end]) {
                    Ok(data) => {
                        let mut inner = FrameSplitter {
                            buf: data,
                            ..Default::default()
                        };
                        self.inflated
                            .extend(std::iter::from_fn(|| inner.next_message()));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "FIXCOMP block that does not inflate was dropped")
                    }
                }
                self.start += end;
                if let Some(m) = self.inflated.pop_front() {
                    return Some(m);
                }
                continue;
            }
            // Locate "10=xxx\x01" after a SOH; bytes before a message that
            // does not start with "8=" are dropped with it.
            let pos = memchr::memmem::find(rest, b"\x0110=")?;
            let end = memchr::memchr(SOH, &rest[pos + 4..])? + pos + 4 + 1;
            let message = rest[..end].to_vec();
            self.start += end;
            if message.starts_with(b"8=") {
                return Some(message);
            }
        }
    }
}

/// UTC timestamp `YYYYMMDD-HH:MM:SS.sss`.
pub fn utc_timestamp(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(t)
        .format("%Y%m%d-%H:%M:%S%.3f")
        .to_string()
}

/// Parse a UTC timestamp `YYYYMMDD-HH:MM:SS` with optional `.sss` (up to
/// nanoseconds); `None` when it is not one.
pub fn parse_utc_timestamp(s: &str) -> Option<std::time::SystemTime> {
    chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d-%H:%M:%S%.f")
        .ok()
        .map(|t| t.and_utc().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_timestamps_round_trip() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_767_225_600_123);
        let text = utc_timestamp(t);
        assert_eq!(text, "20260101-00:00:00.123");
        assert_eq!(parse_utc_timestamp(&text), Some(t));
        assert!(parse_utc_timestamp("20260101-00:00:00").is_some());
        for bad in [
            "2026-01-01",
            "20261301-00:00:00",
            "20260101-00:00:00.",
            "20260101-00:00:00x",
        ] {
            assert_eq!(parse_utc_timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn encode_then_parse_round_trips_and_validates() {
        let mut m = Message::new("D");
        m.push(tags::CL_ORD_ID, "abc")
            .push(tags::SYMBOL, "IBM")
            .push_f64(tags::PRICE, 80.5);
        let mut out = Vec::new();
        m.encode("FIX.4.2", &[(49, "ME"), (56, "IB"), (34, "7")], &mut out);
        let text = String::from_utf8(out.clone()).unwrap().replace('\x01', "|");
        assert!(text.starts_with("8=FIX.4.2|9="), "{text}");
        assert!(
            text.contains("|35=D|49=ME|56=IB|34=7|11=abc|55=IBM|44=80.5|10="),
            "{text}"
        );
        let v = MessageView::parse(&out).unwrap();
        assert_eq!(
            (v.msg_type(), v.get(55), v.seq_num()),
            ("D", Some("IBM"), 7)
        );
        let mut bad = out.clone();
        let n = bad.len();
        bad[n - 2] = if bad[n - 2] == b'0' { b'1' } else { b'0' };
        assert!(matches!(
            MessageView::parse(&bad),
            Err(ParseError::CheckSum { .. })
        ));
    }

    #[test]
    fn known_checksum_vector() {
        // Logon from the FIX specification examples (computed independently).
        let raw = b"8=FIX.4.2\x019=67\x0135=A\x0134=1\x0149=SERVER\x0152=20050520-14:25:29.000\x0156=CLIENT\x0198=0\x01108=30\x0110=140\x01";
        let v = MessageView::parse(raw);
        assert!(v.is_ok(), "{v:?}");
    }

    #[test]
    fn splitter_handles_partial_and_multiple_messages() {
        let mut out = Vec::new();
        Message::new("0").encode("FIX.4.2", &[(34, "1")], &mut out);
        Message::new("0").encode("FIX.4.2", &[(34, "2")], &mut out);
        let mut s = FrameSplitter::default();
        s.extend(&out[..10]);
        assert!(s.next_message().is_none());
        s.extend(&out[10..]);
        assert_eq!(
            MessageView::parse(&s.next_message().unwrap())
                .unwrap()
                .seq_num(),
            1
        );
        assert_eq!(
            MessageView::parse(&s.next_message().unwrap())
                .unwrap()
                .seq_num(),
            2
        );
        assert!(s.next_message().is_none());
    }

    #[test]
    fn fixcomp_blocks_inflate_into_their_messages() {
        let mut plain = Vec::new();
        Message::new("0").encode("FIX.4.2", &[(34, "1")], &mut plain);
        Message::new("0").encode("FIX.4.2", &[(34, "2")], &mut plain);
        let block = fixcomp(&plain);
        assert!(block.starts_with(b"8=FIXCOMP\x019="));
        let mut after = Vec::new();
        Message::new("0").encode("FIX.4.2", &[(34, "3")], &mut after);
        let mut s = FrameSplitter::default();
        s.extend(&block[..block.len() - 3]);
        assert!(s.next_message().is_none(), "a partial block waits");
        s.extend(&block[block.len() - 3..]);
        s.extend(&after);
        let seqs: Vec<u64> = std::iter::from_fn(|| s.next_message())
            .map(|m| MessageView::parse(&m).unwrap().seq_num())
            .collect();
        assert_eq!(seqs, [1, 2, 3]);
    }
}
