//! Minimal protobuf wire format (proto3) for the TWS protobuf messages.
//!
//! [`PbWriter`] writes fields the way IB's client does: a field is written
//! only when it holds a value (not `UNSET_*`, not empty, not `false`), so the
//! server sees "not set" exactly as from IB's own client. [`PbMsg`] reads a
//! message into (field number, value) pairs without copying; accessors look
//! fields up by number. Field numbers live in [`crate::proto`].

use crate::consts::{UNSET_DOUBLE, UNSET_INTEGER, UNSET_LONG};
use crate::types::TagValue;
use crate::wire::DecodeError;
use serde_json::{Map, Value};

/// Field type in the generated [`crate::proto`] tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int32,
    Int64,
    Bool,
    Double,
    String,
    Message,
    Map,
}

/// One field of a protobuf message.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub number: u32,
    pub kind: Kind,
    /// Message type name for `Kind::Message`.
    pub message: &'static str,
    pub repeated: bool,
}

/// Protobuf encoder.
#[derive(Debug, Default)]
pub struct PbWriter {
    pub buf: Vec<u8>,
}

impl PbWriter {
    fn varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.buf.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }

    fn key(&mut self, field: u32, wire_type: u8) {
        self.varint(u64::from(field) << 3 | u64::from(wire_type));
    }

    /// int32 (negative values take 10 bytes, as protobuf specifies).
    pub fn i32(&mut self, field: u32, v: i32) {
        self.key(field, 0);
        self.varint(i64::from(v) as u64);
    }

    pub fn i64(&mut self, field: u32, v: i64) {
        self.key(field, 0);
        self.varint(v as u64);
    }

    pub fn bool(&mut self, field: u32, v: bool) {
        self.key(field, 0);
        self.varint(u64::from(v));
    }

    pub fn f64(&mut self, field: u32, v: f64) {
        self.key(field, 1);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn str(&mut self, field: u32, v: &str) {
        self.key(field, 2);
        self.varint(v.len() as u64);
        self.buf.extend_from_slice(v.as_bytes());
    }

    /// Nested message built by `build`.
    pub fn msg(&mut self, field: u32, build: impl FnOnce(&mut PbWriter)) {
        let mut inner = PbWriter::default();
        build(&mut inner);
        self.key(field, 2);
        self.varint(inner.buf.len() as u64);
        self.buf.extend_from_slice(&inner.buf);
    }

    /// int32 unless `UNSET_INTEGER`.
    pub fn opt_i32(&mut self, field: u32, v: i32) {
        if v != UNSET_INTEGER {
            self.i32(field, v);
        }
    }

    /// int64 unless `UNSET_LONG`.
    pub fn opt_i64(&mut self, field: u32, v: i64) {
        if v != UNSET_LONG {
            self.i64(field, v);
        }
    }

    /// double unless `UNSET_DOUBLE`.
    pub fn opt_f64(&mut self, field: u32, v: f64) {
        if v != UNSET_DOUBLE {
            self.f64(field, v);
        }
    }

    /// Decimal quantity as its string form, unless `UNSET_DOUBLE`.
    pub fn opt_decimal(&mut self, field: u32, v: f64) {
        if v != UNSET_DOUBLE {
            let mut s = Vec::new();
            crate::wire::write_f64(&mut s, v);
            self.str(field, std::str::from_utf8(&s).unwrap_or("0"));
        }
    }

    /// string unless empty.
    pub fn opt_str(&mut self, field: u32, v: &str) {
        if !v.is_empty() {
            self.str(field, v);
        }
    }

    /// bool when true.
    pub fn opt_bool(&mut self, field: u32, v: bool) {
        if v {
            self.bool(field, true);
        }
    }

    /// Packed repeated int32.
    pub fn packed_i32(&mut self, field: u32, values: &[i32]) {
        if values.is_empty() {
            return;
        }
        let mut inner = PbWriter::default();
        for v in values {
            inner.varint(i64::from(*v) as u64);
        }
        self.key(field, 2);
        self.varint(inner.buf.len() as u64);
        self.buf.extend_from_slice(&inner.buf);
    }

    /// `map<string, string>` from tag/value pairs.
    pub fn map(&mut self, field: u32, items: &[TagValue]) {
        for tv in items {
            self.msg(field, |w| {
                w.str(1, &tv.tag);
                w.str(2, &tv.value);
            });
        }
    }

    /// Frame: 4-byte length, 4-byte message id (`msg_id` + 200), payload.
    pub fn frame(&self, msg_id: i32) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.buf.len());
        out.extend_from_slice(&((self.buf.len() + 4) as u32).to_be_bytes());
        out.extend_from_slice(&(msg_id + crate::message::PROTOBUF_MSG_ID).to_be_bytes());
        out.extend_from_slice(&self.buf);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Val<'a> {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32),
    Bytes(&'a [u8]),
}

/// A parsed protobuf message; repeated fields keep every occurrence in order.
#[derive(Clone, Debug, Default)]
pub struct PbMsg<'a> {
    fields: Vec<(u32, Val<'a>)>,
}

fn bad(what: &str) -> DecodeError {
    DecodeError::Protobuf(what.into())
}

fn read_varint(b: &[u8], pos: &mut usize) -> Result<u64, DecodeError> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*pos).ok_or_else(|| bad("varint runs past the end"))?;
        *pos += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Ok(v);
        }
    }
    Err(bad("varint longer than 10 bytes"))
}

impl<'a> PbMsg<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, DecodeError> {
        let mut fields = Vec::with_capacity(16);
        let mut pos = 0;
        while pos < b.len() {
            let key = read_varint(b, &mut pos)?;
            let field = (key >> 3) as u32;
            let take = |pos: &mut usize, n: usize| -> Result<&'a [u8], DecodeError> {
                let s = b
                    .get(*pos..*pos + n)
                    .ok_or_else(|| bad("field runs past the end"))?;
                *pos += n;
                Ok(s)
            };
            let val = match key & 7 {
                0 => Val::Varint(read_varint(b, &mut pos)?),
                1 => Val::Fixed64(u64::from_le_bytes(
                    take(&mut pos, 8)?.try_into().unwrap_or_default(),
                )),
                2 => {
                    let n = read_varint(b, &mut pos)? as usize;
                    Val::Bytes(take(&mut pos, n)?)
                }
                5 => Val::Fixed32(u32::from_le_bytes(
                    take(&mut pos, 4)?.try_into().unwrap_or_default(),
                )),
                t => {
                    return Err(DecodeError::Protobuf(format!(
                        "unsupported wire type {t} for field {field}"
                    )));
                }
            };
            fields.push((field, val));
        }
        Ok(Self { fields })
    }

    fn last(&self, field: u32) -> Option<Val<'a>> {
        self.fields
            .iter()
            .rev()
            .find(|(f, _)| *f == field)
            .map(|(_, v)| *v)
    }

    fn all(&self, field: u32) -> impl Iterator<Item = Val<'a>> + '_ {
        self.fields
            .iter()
            .filter(move |(f, _)| *f == field)
            .map(|(_, v)| *v)
    }

    pub fn has(&self, field: u32) -> bool {
        self.last(field).is_some()
    }

    pub fn i64(&self, field: u32) -> Option<i64> {
        match self.last(field)? {
            Val::Varint(v) => Some(v as i64),
            _ => None,
        }
    }

    pub fn i32(&self, field: u32) -> Option<i32> {
        self.i64(field).map(|v| v as i32)
    }

    pub fn bool(&self, field: u32) -> Option<bool> {
        self.i64(field).map(|v| v != 0)
    }

    pub fn f64(&self, field: u32) -> Option<f64> {
        match self.last(field)? {
            Val::Fixed64(v) => Some(f64::from_bits(v)),
            Val::Fixed32(v) => Some(f64::from(f32::from_bits(v))),
            _ => None,
        }
    }

    pub fn str(&self, field: u32) -> Option<&'a str> {
        match self.last(field)? {
            Val::Bytes(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    pub fn msg(&self, field: u32) -> Option<PbMsg<'a>> {
        match self.last(field)? {
            Val::Bytes(b) => PbMsg::parse(b).ok(),
            _ => None,
        }
    }

    // ---- with IB's defaults for absent fields ----

    /// int32, `UNSET_INTEGER` when absent.
    pub fn int(&self, field: u32) -> i32 {
        self.i32(field).unwrap_or(UNSET_INTEGER)
    }

    /// int32, `default` when absent.
    pub fn int_or(&self, field: u32, default: i32) -> i32 {
        self.i32(field).unwrap_or(default)
    }

    /// int64, `UNSET_LONG` when absent.
    pub fn long(&self, field: u32) -> i64 {
        self.i64(field).unwrap_or(UNSET_LONG)
    }

    /// double, `UNSET_DOUBLE` when absent.
    pub fn double(&self, field: u32) -> f64 {
        self.f64(field).unwrap_or(UNSET_DOUBLE)
    }

    /// Decimal sent as a string, `UNSET_DOUBLE` when absent or not a number.
    pub fn decimal(&self, field: u32) -> f64 {
        self.str(field)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(UNSET_DOUBLE)
    }

    /// bool, false when absent.
    pub fn flag(&self, field: u32) -> bool {
        self.bool(field).unwrap_or(false)
    }

    /// string, empty when absent.
    pub fn string(&self, field: u32) -> String {
        self.str(field).unwrap_or_default().to_string()
    }

    /// Every occurrence of a repeated message field.
    pub fn msgs(&self, field: u32) -> impl Iterator<Item = PbMsg<'a>> + '_ {
        self.all(field).filter_map(|v| match v {
            Val::Bytes(b) => PbMsg::parse(b).ok(),
            _ => None,
        })
    }

    pub fn strings(&self, field: u32) -> Vec<String> {
        self.all(field)
            .filter_map(|v| match v {
                Val::Bytes(b) => std::str::from_utf8(b).ok().map(String::from),
                _ => None,
            })
            .collect()
    }

    /// Repeated double, packed or not.
    pub fn doubles(&self, field: u32) -> Vec<f64> {
        let mut out = Vec::new();
        for v in self.all(field) {
            match v {
                Val::Fixed64(x) => out.push(f64::from_bits(x)),
                Val::Bytes(b) => {
                    out.extend(b.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)))
                }
                _ => {}
            }
        }
        out
    }

    /// Repeated int32/int64, packed or not.
    pub fn longs(&self, field: u32) -> Vec<i64> {
        let mut out = Vec::new();
        for v in self.all(field) {
            match v {
                Val::Varint(x) => out.push(x as i64),
                Val::Bytes(b) => {
                    let mut pos = 0;
                    while pos < b.len() {
                        match read_varint(b, &mut pos) {
                            Ok(x) => out.push(x as i64),
                            Err(_) => break,
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// `map<string, string>` as tag/value pairs.
    pub fn map(&self, field: u32) -> Vec<TagValue> {
        self.msgs(field)
            .map(|e| TagValue {
                tag: e.string(1),
                value: e.string(2),
            })
            .collect()
    }
}

/// JSON form of message `name` (field names as in the .proto files; maps
/// become objects, repeated fields arrays). Unknown fields are skipped.
pub fn to_json(name: &str, m: &PbMsg<'_>) -> Value {
    let Some(fields) = crate::proto::fields(name) else {
        return Value::Null;
    };
    let mut out = Map::new();
    for f in fields {
        if !m.has(f.number) {
            continue;
        }
        let one = |v: Val<'_>| -> Value {
            match (f.kind, v) {
                (Kind::Int32, Val::Varint(x)) => Value::from(x as i64 as i32),
                (Kind::Int64, Val::Varint(x)) => Value::from(x as i64),
                (Kind::Bool, Val::Varint(x)) => Value::from(x != 0),
                (Kind::Double, Val::Fixed64(x)) => serde_json::Number::from_f64(f64::from_bits(x))
                    .map_or(Value::Null, Value::Number),
                (Kind::String, Val::Bytes(b)) => {
                    Value::from(String::from_utf8_lossy(b).into_owned())
                }
                (Kind::Message, Val::Bytes(b)) => {
                    PbMsg::parse(b).map_or(Value::Null, |inner| to_json(f.message, &inner))
                }
                _ => Value::Null,
            }
        };
        let value = match f.kind {
            Kind::Map => Value::Object(
                m.map(f.number)
                    .into_iter()
                    .map(|tv| (tv.tag, Value::from(tv.value)))
                    .collect(),
            ),
            Kind::Double if f.repeated => Value::from(m.doubles(f.number)),
            Kind::Int32 | Kind::Int64 | Kind::Bool if f.repeated => Value::Array(
                m.longs(f.number)
                    .into_iter()
                    .map(|x| match f.kind {
                        Kind::Bool => Value::from(x != 0),
                        Kind::Int32 => Value::from(x as i32),
                        _ => Value::from(x),
                    })
                    .collect(),
            ),
            _ if f.repeated => Value::Array(m.all(f.number).map(one).collect()),
            _ => m.last(f.number).map_or(Value::Null, one),
        };
        out.insert(f.name.to_string(), value);
    }
    Value::Object(out)
}

/// Write the JSON form of message `name` (as produced by [`to_json`]).
/// Fields that are not in the message or have the wrong JSON type are skipped.
pub fn from_json(w: &mut PbWriter, name: &str, v: &Value) {
    let (Some(fields), Some(obj)) = (crate::proto::fields(name), v.as_object()) else {
        return;
    };
    for f in fields {
        let Some(value) = obj.get(f.name) else {
            continue;
        };
        let items: Vec<&Value> = match (f.repeated, value) {
            (true, Value::Array(a)) => a.iter().collect(),
            _ => vec![value],
        };
        for item in items {
            match f.kind {
                Kind::Int32 => {
                    if let Some(x) = item.as_i64() {
                        w.i32(f.number, x as i32);
                    }
                }
                Kind::Int64 => {
                    if let Some(x) = item.as_i64() {
                        w.i64(f.number, x);
                    }
                }
                Kind::Bool => {
                    if let Some(x) = item.as_bool() {
                        w.bool(f.number, x);
                    }
                }
                Kind::Double => {
                    if let Some(x) = item.as_f64() {
                        w.f64(f.number, x);
                    }
                }
                Kind::String => {
                    if let Some(x) = item.as_str() {
                        w.str(f.number, x);
                    }
                }
                Kind::Message => w.msg(f.number, |w| from_json(w, f.message, item)),
                Kind::Map => {
                    if let Some(map) = item.as_object() {
                        for (k, val) in map {
                            let text = val.as_str().map_or_else(|| val.to_string(), String::from);
                            w.msg(f.number, |w| {
                                w.str(1, k);
                                w.str(2, &text);
                            });
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_and_reader_round_trip() {
        let mut w = PbWriter::default();
        w.i32(1, -5);
        w.opt_i32(2, UNSET_INTEGER);
        w.f64(3, 1.5);
        w.str(4, "abc");
        w.opt_str(5, "");
        w.opt_bool(6, true);
        w.msg(7, |n| n.i64(1, 1 << 40));
        w.map(
            8,
            &[TagValue {
                tag: "k".into(),
                value: "v".into(),
            }],
        );
        w.opt_decimal(9, 100.0);
        // packed repeated double [1.0, 2.0]
        w.key(10, 2);
        w.varint(16);
        w.buf.extend_from_slice(&1.0f64.to_le_bytes());
        w.buf.extend_from_slice(&2.0f64.to_le_bytes());
        let m = PbMsg::parse(&w.buf).unwrap();
        assert_eq!(m.i32(1), Some(-5));
        assert_eq!(m.int(2), UNSET_INTEGER);
        assert_eq!(m.double(3), 1.5);
        assert_eq!(m.string(4), "abc");
        assert!(!m.has(5));
        assert!(m.flag(6));
        assert_eq!(m.msg(7).unwrap().i64(1), Some(1 << 40));
        assert_eq!(m.map(8)[0].value, "v");
        assert_eq!(m.decimal(9), 100.0);
        assert_eq!(m.doubles(10), [1.0, 2.0]);
        assert!(PbMsg::parse(&[0x0a, 0x05, 0x01]).is_err());
        assert_eq!(&w.frame(1)[4..8], &201i32.to_be_bytes());
    }

    #[test]
    fn json_round_trip_follows_the_schema() {
        let v = serde_json::json!({"reqId": 3, "tickType": 1, "price": 1.25, "size": "100", "attrMask": 2});
        let mut w = PbWriter::default();
        from_json(&mut w, "TickPrice", &v);
        assert_eq!(to_json("TickPrice", &PbMsg::parse(&w.buf).unwrap()), v);
        let nested = serde_json::json!({"reqId": 1, "contract": {"conId": 5, "symbol": "X", "comboLegs": [{"conId": 1}, {"conId": 2}]}, "marketDataOptions": {"a": "b"}});
        let mut w = PbWriter::default();
        from_json(&mut w, "MarketDataRequest", &nested);
        assert_eq!(
            to_json("MarketDataRequest", &PbMsg::parse(&w.buf).unwrap()),
            nested
        );
    }
}
