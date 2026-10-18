//! Reader for Android Binary XML (ABX, Android 12+), the format of many files in /data/system.
//! Own parser because `abx2xml` stops halfway when a value contains U+0000
//! (backup-android.md, "XML binario (ABX)"). Format: frameworks/base BinaryXmlSerializer.

use std::fmt::Write as _;

use crate::{Error, Result};

pub const MAGIC: &[u8; 4] = b"ABX\0";

// Token = command (low 4 bits) | type (high 4 bits).
const START_DOCUMENT: u8 = 0;
const END_DOCUMENT: u8 = 1;
const START_TAG: u8 = 2;
const END_TAG: u8 = 3;
const TEXT: u8 = 4;
const CDSECT: u8 = 5;
const ENTITY_REF: u8 = 6;
const IGNORABLE_WHITESPACE: u8 = 7;
const PROCESSING_INSTRUCTION: u8 = 8;
const COMMENT: u8 = 9;
const DOCDECL: u8 = 10;
const ATTRIBUTE: u8 = 15;

const TYPE_NULL: u8 = 1 << 4;
const TYPE_STRING: u8 = 2 << 4;
const TYPE_STRING_INTERNED: u8 = 3 << 4;
const TYPE_BYTES_HEX: u8 = 4 << 4;
const TYPE_BYTES_BASE64: u8 = 5 << 4;
const TYPE_INT: u8 = 6 << 4;
const TYPE_INT_HEX: u8 = 7 << 4;
const TYPE_LONG: u8 = 8 << 4;
const TYPE_LONG_HEX: u8 = 9 << 4;
const TYPE_FLOAT: u8 = 10 << 4;
const TYPE_DOUBLE: u8 = 11 << 4;
const TYPE_BOOLEAN_TRUE: u8 = 12 << 4;
const TYPE_BOOLEAN_FALSE: u8 = 13 << 4;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    String(String),
    Bytes(Vec<u8>),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Bool(bool),
}

impl Value {
    /// The value as XML text, the way `abx2xml` prints it.
    pub fn to_text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Bytes(b) => crate::stream::hex(b),
            Value::Int(i) => i.to_string(),
            Value::Long(l) => l.to_string(),
            Value::Float(f) => java_float(*f as f64),
            Value::Double(d) => java_float(*d),
            Value::Bool(b) => b.to_string(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, Value)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&Value> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Every descendant element called `name`, depth first.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for e in self.elements() {
            if e.name == name {
                out.push(e);
            }
            e.find_all(name, out);
        }
    }
}

/// Like Java's `Float/Double.toString` for ordinary values: always a decimal point ("1.0").
fn java_float(v: f64) -> String {
    let s = v.to_string();
    if v.is_finite() && !s.contains(['.', 'e', 'E']) { format!("{s}.0") } else { s }
}

pub fn is_abx(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    interned: Vec<String>,
}

impl<'a> Reader<'a> {
    fn err(&self, what: &str) -> Error {
        Error::UnexpectedOutput { what: format!("ABX at byte {}: {what}", self.pos) }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(|| self.err("truncated"))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// `DataOutput.writeUTF`: u16 length + modified UTF-8.
    fn utf(&mut self) -> Result<String> {
        let len = self.u16()? as usize;
        Ok(decode_modified_utf8(self.take(len)?))
    }

    fn interned(&mut self) -> Result<String> {
        let index = self.u16()?;
        if index == 0xFFFF {
            let s = self.utf()?;
            self.interned.push(s.clone());
            Ok(s)
        } else {
            self.interned.get(index as usize).cloned().ok_or_else(|| self.err("bad interned index"))
        }
    }

    fn value(&mut self, ty: u8) -> Result<Value> {
        Ok(match ty {
            TYPE_NULL => Value::Null,
            TYPE_STRING => Value::String(self.utf()?),
            TYPE_STRING_INTERNED => Value::String(self.interned()?),
            TYPE_BYTES_HEX | TYPE_BYTES_BASE64 => {
                let len = self.u16()? as usize;
                Value::Bytes(self.take(len)?.to_vec())
            }
            TYPE_INT | TYPE_INT_HEX => Value::Int(self.i32()?),
            TYPE_LONG | TYPE_LONG_HEX => Value::Long(self.i64()?),
            TYPE_FLOAT => Value::Float(f32::from_bits(self.i32()? as u32)),
            TYPE_DOUBLE => Value::Double(f64::from_bits(self.i64()? as u64)),
            TYPE_BOOLEAN_TRUE => Value::Bool(true),
            TYPE_BOOLEAN_FALSE => Value::Bool(false),
            _ => return Err(self.err(&format!("unknown type {ty:#x}"))),
        })
    }
}

/// Java's modified UTF-8 (U+0000 as C0 80, supplementary characters as surrogate pairs).
/// Lone surrogates and invalid bytes become U+FFFD.
fn decode_modified_utf8(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (unit, len) = if b < 0x80 {
            (b as u16, 1)
        } else if b & 0xE0 == 0xC0 && i + 1 < bytes.len() {
            ((((b & 0x1F) as u16) << 6) | (bytes[i + 1] & 0x3F) as u16, 2)
        } else if b & 0xF0 == 0xE0 && i + 2 < bytes.len() {
            ((((b & 0x0F) as u16) << 12) | (((bytes[i + 1] & 0x3F) as u16) << 6) | (bytes[i + 2] & 0x3F) as u16, 3)
        } else {
            (0xFFFD, 1)
        };
        units.push(unit);
        i += len;
    }
    String::from_utf16_lossy(&units)
}

/// Parses an ABX document into its root element (the first top-level one).
pub fn parse(data: &[u8]) -> Result<Element> {
    parse_all(data)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::UnexpectedOutput { what: "ABX without a root element".into() })
}

/// All top-level elements: some files have more than one (settings_ssaid.xml ends with a
/// stray `<namespaceHashes />` after `</settings>`).
pub fn parse_all(data: &[u8]) -> Result<Vec<Element>> {
    if !is_abx(data) {
        return Err(Error::UnexpectedOutput { what: "not ABX".into() });
    }
    let mut r = Reader { data, pos: MAGIC.len(), interned: Vec::new() };
    // Open elements; index 0 is a synthetic document node.
    let mut stack: Vec<Element> = vec![Element::default()];
    while r.pos < r.data.len() {
        let token = r.u8()?;
        let (command, ty) = (token & 0x0F, token & 0xF0);
        match command {
            START_DOCUMENT => {}
            END_DOCUMENT => break,
            START_TAG => {
                let name = r.interned()?;
                stack.push(Element { name, ..Default::default() });
            }
            ATTRIBUTE => {
                let name = r.interned()?;
                let value = r.value(ty)?;
                stack.last_mut().ok_or_else(|| r.err("attribute outside a tag"))?.attrs.push((name, value));
            }
            END_TAG => {
                let _name = r.interned()?;
                if stack.len() < 2 {
                    return Err(r.err("unbalanced end tag"));
                }
                let done = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(Node::Element(done));
            }
            TEXT | CDSECT | ENTITY_REF | IGNORABLE_WHITESPACE | PROCESSING_INSTRUCTION | COMMENT | DOCDECL => {
                let text = r.value(ty)?.to_text();
                if command == TEXT || command == CDSECT {
                    stack.last_mut().unwrap().children.push(Node::Text(text));
                }
            }
            _ => return Err(r.err(&format!("unknown command {command}"))),
        }
    }
    if stack.len() != 1 {
        return Err(r.err("unclosed elements"));
    }
    let doc = stack.into_iter().next().unwrap();
    Ok(doc
        .children
        .into_iter()
        .filter_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
        .collect())
}

/// Text XML for viewing (Mis copias). U+0000 and other characters not allowed in XML are
/// written as `&#xN;` so nothing is lost.
pub fn to_xml(elements: &[Element]) -> String {
    let mut out = String::from("<?xml version='1.0' encoding='utf-8' standalone='yes' ?>\n");
    for e in elements {
        write_element(&mut out, e, 0);
    }
    out
}

fn write_element(out: &mut String, e: &Element, depth: usize) {
    let indent = "  ".repeat(depth);
    let _ = write!(out, "{indent}<{}", e.name);
    for (k, v) in &e.attrs {
        let _ = write!(out, " {k}=\"{}\"", escape(&v.to_text()));
    }
    if e.children.is_empty() {
        out.push_str(" />\n");
        return;
    }
    out.push_str(">\n");
    for c in &e.children {
        match c {
            Node::Element(child) => write_element(out, child, depth + 1),
            Node::Text(t) if !t.trim().is_empty() => {
                let _ = writeln!(out, "{indent}  {}", escape(t));
            }
            Node::Text(_) => {}
        }
    }
    let _ = writeln!(out, "{indent}</{}>", e.name);
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\t' | '\n' | '\r' => o.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {
                let _ = write!(o, "&#x{:X};", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal writer mirroring BinaryXmlSerializer, for tests.
    struct W {
        out: Vec<u8>,
        interned: Vec<String>,
    }

    impl W {
        fn new() -> Self {
            let mut out = MAGIC.to_vec();
            out.push(START_DOCUMENT | TYPE_NULL);
            W { out, interned: vec![] }
        }
        fn utf(&mut self, s: &str) {
            let mut b = Vec::new();
            for unit in s.encode_utf16() {
                match unit {
                    0 => b.extend([0xC0, 0x80]),
                    1..=0x7F => b.push(unit as u8),
                    0x80..=0x7FF => b.extend([0xC0 | (unit >> 6) as u8, 0x80 | (unit & 0x3F) as u8]),
                    _ => b.extend([0xE0 | (unit >> 12) as u8, 0x80 | ((unit >> 6) & 0x3F) as u8, 0x80 | (unit & 0x3F) as u8]),
                }
            }
            self.out.extend((b.len() as u16).to_be_bytes());
            self.out.extend(b);
        }
        fn interned(&mut self, s: &str) {
            if let Some(i) = self.interned.iter().position(|x| x == s) {
                self.out.extend((i as u16).to_be_bytes());
            } else {
                self.out.extend(0xFFFFu16.to_be_bytes());
                self.utf(s);
                self.interned.push(s.to_string());
            }
        }
        fn start(&mut self, name: &str) {
            self.out.push(START_TAG | TYPE_STRING_INTERNED);
            self.interned(name);
        }
        fn end(&mut self, name: &str) {
            self.out.push(END_TAG | TYPE_STRING_INTERNED);
            self.interned(name);
        }
        fn attr_str(&mut self, name: &str, v: &str) {
            self.out.push(ATTRIBUTE | TYPE_STRING);
            self.interned(name);
            self.utf(v);
        }
        fn attr_int(&mut self, name: &str, v: i32) {
            self.out.push(ATTRIBUTE | TYPE_INT);
            self.interned(name);
            self.out.extend(v.to_be_bytes());
        }
        fn attr_bool(&mut self, name: &str, v: bool) {
            self.out.push(ATTRIBUTE | if v { TYPE_BOOLEAN_TRUE } else { TYPE_BOOLEAN_FALSE });
            self.interned(name);
        }
        fn text(&mut self, s: &str) {
            self.out.push(TEXT | TYPE_STRING);
            self.utf(s);
        }
        fn finish(mut self) -> Vec<u8> {
            self.out.push(END_DOCUMENT | TYPE_NULL);
            self.out
        }
    }

    #[test]
    fn parses_tags_attributes_and_nul_in_values() {
        let mut w = W::new();
        w.start("notification-policy");
        w.attr_int("version", 1);
        w.start("channel");
        w.attr_str("name", "Canal\0con nulo ñ 😀");
        w.attr_bool("show_badge", true);
        w.end("channel");
        w.start("channel");
        w.attr_str("name", "Otro");
        w.text("texto");
        w.end("channel");
        w.end("notification-policy");
        let root = parse(&w.finish()).unwrap();
        assert_eq!(root.name, "notification-policy");
        assert_eq!(root.attr("version"), Some(&Value::Int(1)));
        let mut channels = vec![];
        root.find_all("channel", &mut channels);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].attr("name").unwrap().as_str(), Some("Canal\0con nulo ñ 😀"));
        assert_eq!(channels[0].attr("show_badge"), Some(&Value::Bool(true)));
        let xml = to_xml(std::slice::from_ref(&root));
        assert!(xml.contains("name=\"Canal&#x0;con nulo ñ 😀\""), "{xml}");
        assert!(xml.contains("texto"));
    }

    #[test]
    fn keeps_every_top_level_element() {
        let mut w = W::new();
        w.start("settings");
        w.end("settings");
        w.start("namespaceHashes");
        w.end("namespaceHashes");
        let all = parse_all(&w.finish()).unwrap();
        assert_eq!(all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["settings", "namespaceHashes"]);
    }

    #[test]
    fn rejects_truncated_input() {
        let mut w = W::new();
        w.start("a");
        w.attr_str("k", "value");
        let mut bytes = w.finish();
        bytes.truncate(bytes.len() - 4);
        assert!(parse(&bytes).is_err());
        assert!(parse(b"<?xml").is_err());
    }
}
