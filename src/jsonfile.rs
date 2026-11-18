//! A JSONC syntax tree with byte spans, so edits need not reformat the file.
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::Value;
use std::{
    fs,
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
};

struct Node {
    span: Range<usize>,
    kind: Kind,
}

enum Kind {
    Scalar(Value),
    Object(Vec<Member>),
    Array(Vec<Node>),
}

struct Member {
    key: String,
    start: usize,
    value: Node,
}

impl Node {
    fn value(&self) -> Value {
        match &self.kind {
            Kind::Scalar(v) => v.clone(),
            Kind::Object(ms) => Value::Object(
                ms.iter()
                    .map(|m| (m.key.clone(), m.value.value()))
                    .collect(),
            ),
            Kind::Array(ns) => Value::Array(ns.iter().map(Node::value).collect()),
        }
    }

    fn find(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self, |n, &part| match &n.kind {
            Kind::Object(ms) => ms.iter().find(|m| m.key == part).map(|m| &m.value),
            Kind::Array(ns) => ns.get(part.trim().parse::<usize>().ok()?),
            Kind::Scalar(_) => None,
        })
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn trivia(&mut self) {
        loop {
            let rest = self.src[self.pos..].trim_start();
            self.pos = self.src.len() - rest.len();
            if rest.starts_with("//") || rest.starts_with('#') {
                self.pos += rest.find('\n').unwrap_or(rest.len());
            } else if let Some(comment) = rest.strip_prefix("/*") {
                self.pos += comment.find("*/").map_or(rest.len(), |n| n + 4);
            } else {
                break;
            }
        }
    }

    fn take(&mut self, c: u8) -> bool {
        if self.src.as_bytes().get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> Result<String> {
        ensure!(
            self.src[self.pos..].starts_with('"'),
            "expected string at {}",
            self.pos
        );
        let mut stream =
            serde_json::Deserializer::from_str(&self.src[self.pos..]).into_iter::<String>();
        let s = stream.next().ok_or_else(|| anyhow!("unexpected EOF"))??;
        self.pos += stream.byte_offset();
        Ok(s)
    }

    fn node(&mut self, depth: usize) -> Result<Node> {
        ensure!(depth < 256, "JSONC nesting too deep");
        self.trivia();
        let start = self.pos;
        let kind = match self.src.as_bytes().get(start) {
            Some(b'{' | b'[') => {
                let object = self.src.as_bytes()[start] == b'{';
                let close = if object { b'}' } else { b']' };
                self.pos += 1;
                let (mut ms, mut ns) = (Vec::new(), Vec::new());
                loop {
                    self.trivia();
                    if self.take(close) {
                        break;
                    }
                    if object {
                        let start = self.pos;
                        let key = self.string()?;
                        self.trivia();
                        ensure!(self.take(b':'), "expected ':' at {}", self.pos);
                        ms.push(Member {
                            key,
                            start,
                            value: self.node(depth + 1)?,
                        });
                    } else {
                        ns.push(self.node(depth + 1)?);
                    }
                    self.trivia();
                    if self.take(close) {
                        break;
                    }
                    ensure!(self.take(b','), "expected separator at {}", self.pos);
                }
                if object {
                    Kind::Object(ms)
                } else {
                    Kind::Array(ns)
                }
            }
            Some(b'"') => Kind::Scalar(Value::String(self.string()?)),
            Some(_) => {
                let rest = &self.src[start..];
                let end = rest
                    .char_indices()
                    .find(|&(i, c)| {
                        matches!(c, ',' | '}' | ']' | '#')
                            || rest[i..].starts_with("//")
                            || rest[i..].starts_with("/*")
                    })
                    .map_or(rest.len(), |(i, _)| i);
                let raw = rest[..end].trim_end();
                ensure!(!raw.is_empty(), "expected value at {start}");
                self.pos = start + raw.len();
                Kind::Scalar(
                    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into())),
                )
            }
            None => bail!("unexpected end of file"),
        };
        Ok(Node {
            span: start..self.pos,
            kind,
        })
    }
}

// Match json.dumps(..., ensure_ascii=False) separators in inserted values.
fn dump(v: &Value) -> String {
    match v {
        Value::Object(ms) => format!(
            "{{{}}}",
            ms.iter()
                .map(|(k, v)| format!("{}: {}", serde_json::to_string(k).unwrap(), dump(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(ns) => format!("[{}]", ns.iter().map(dump).collect::<Vec<_>>().join(", ")),
        _ => v.to_string(),
    }
}

pub struct JsonFile {
    path: PathBuf,
    src: String,
    root: Node,
    eol: &'static str,
    bom: bool,
}

impl JsonFile {
    pub fn open(path: &Path, create: bool) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if create && e.kind() == ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let bom = raw.starts_with('\u{feff}');
        let mut src = raw.strip_prefix('\u{feff}').unwrap_or(&raw).to_owned();
        let mut parser = Parser { src: &src, pos: 0 };
        parser.trivia();
        if parser.pos == src.len() {
            src = "{}".into();
        }
        let root = Parser { src: &src, pos: 0 }.node(0)?;
        let eol = if src.contains("\r\n") { "\r\n" } else { "\n" };
        Ok(Self {
            path: path.into(),
            src,
            root,
            eol,
            bom,
        })
    }

    pub fn get(&self, path: &[&str]) -> Option<Value> {
        self.root.find(path).map(Node::value)
    }

    fn indent(&self, pos: usize) -> &str {
        let line = self.src[..pos].rsplit('\n').next().unwrap();
        &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
    }

    fn comma_after(&self, pos: usize) -> Option<usize> {
        let mut p = Parser {
            src: &self.src,
            pos,
        };
        p.trivia();
        (self.src.as_bytes().get(p.pos) == Some(&b',')).then_some(p.pos)
    }

    fn splice(&mut self, range: Range<usize>, text: &str) -> Result<()> {
        let mut src = self.src.clone();
        src.replace_range(range, text);
        self.root = Parser { src: &src, pos: 0 }.node(0)?;
        self.src = src;
        Ok(())
    }

    // Insert an absent object key, or grow an array through the requested index.
    fn insert(&mut self, path: &[&str], key: &str, value: &str, filler: &str) -> Result<()> {
        let parent = self
            .root
            .find(path)
            .ok_or_else(|| anyhow!("missing parent"))?;
        let nl = self.eol;
        let outer = self.indent(parent.span.start);
        let (pos, text) = match &parent.kind {
            Kind::Object(ms) => {
                let key = serde_json::to_string(key)?;
                if let Some(last) = ms.last() {
                    let end = last.value.span.end;
                    let (pos, sep) = self.comma_after(end).map_or((end, ","), |i| (i + 1, ""));
                    let mut indent = self.indent(last.start).to_owned();
                    if indent.is_empty() {
                        indent = format!("{outer}  ");
                    }
                    (pos, format!("{sep}{nl}{indent}{key}: {value}"))
                } else {
                    (
                        parent.span.start + 1,
                        format!("{nl}{outer}  {key}: {value}{nl}{outer}"),
                    )
                }
            }
            Kind::Array(ns) => {
                let index = key.trim().parse::<usize>()?;
                let pos = parent.span.end - 1;
                let mut text = String::new();
                if ns
                    .last()
                    .is_some_and(|last| self.comma_after(last.span.end).is_none())
                {
                    text.push(',');
                }
                for _ in ns.len()..index {
                    text.push_str(&format!("{nl}{outer}  {filler},"));
                }
                text.push_str(&format!("{nl}{outer}  {value}"));
                (pos, text)
            }
            Kind::Scalar(_) => bail!("cannot set {key:?} in a scalar"),
        };
        self.splice(pos..pos, &text)
    }

    pub fn set(&mut self, path: &[&str], value: Value) -> Result<()> {
        let (&key, parents) = path.split_last().ok_or_else(|| anyhow!("empty path"))?;
        for (i, part) in parents.iter().enumerate() {
            if self.root.find(&path[..=i]).is_none() {
                self.insert(&path[..i], part, "{}", "{}")?;
            }
        }
        let text = dump(&value);
        if let Some(node) = self.root.find(path) {
            self.splice(node.span.clone(), &text)
        } else {
            self.insert(parents, key, &text, "null")
        }
    }

    pub fn delete(&mut self, path: &[&str]) -> Result<()> {
        let (&key, parents) = path.split_last().ok_or_else(|| anyhow!("empty path"))?;
        let Some(parent) = self.root.find(parents) else {
            return Ok(());
        };
        let (mut start, mut end, prev) = match &parent.kind {
            Kind::Object(ms) => {
                let Some(i) = ms.iter().position(|m| m.key == key) else {
                    return Ok(());
                };
                (
                    ms[i].start,
                    ms[i].value.span.end,
                    i.checked_sub(1).map(|j| ms[j].value.span.end),
                )
            }
            Kind::Array(ns) => {
                let Ok(i) = key.trim().parse::<usize>() else {
                    return Ok(());
                };
                let Some(n) = ns.get(i) else { return Ok(()) };
                (
                    n.span.start,
                    n.span.end,
                    i.checked_sub(1).map(|j| ns[j].span.end),
                )
            }
            Kind::Scalar(_) => return Ok(()),
        };
        if let Some(comma) = self.comma_after(end) {
            end = comma + 1;
        } else if let Some(comma) = prev.and_then(|p| self.comma_after(p)) {
            start = comma;
        }
        if matches!(parent.kind, Kind::Object(_)) {
            start = self.src[..start].trim_end_matches([' ', '\t']).len();
        }
        self.splice(start..end, "")
    }

    pub fn save(&self) -> Result<bool> {
        match fs::read_to_string(&self.path) {
            Ok(s) if s.strip_prefix('\u{feff}').unwrap_or(&s) == self.src => return Ok(false),
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let mut data = Vec::new();
        if self.bom {
            data.extend_from_slice(b"\xef\xbb\xbf");
        }
        data.extend_from_slice(self.src.as_bytes());
        crate::write_atomic(&self.path, &data)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::{TempDir, tempdir};

    fn fixture(src: &str) -> (TempDir, JsonFile) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, src).unwrap();
        let f = JsonFile::open(&path, false).unwrap();
        (dir, f)
    }

    #[test]
    fn surgical_nested_edits() -> Result<()> {
        let (_dir, mut f) = fixture(
            "{\n  // keep this comment\n  \"model\": \"old/m\",\n  \"provider\": {\n    // provider comment\n    \"mygw\": {\"options\": {\"baseURL\": \"x\"}}\n  }\n}\n",
        );
        let url = ["provider", "mygw", "options", "baseURL"];
        let headers = ["provider", "mygw", "options", "headers"];
        assert_eq!(f.get(&["model"]), Some(json!("old/m")));
        assert_eq!(f.get(&url), Some(json!("x")));
        f.set(&["model"], json!("mygw/m2"))?;
        f.set(&url, json!("http://new/v1"))?;
        f.set(&headers, json!({"a":1}))?;
        f.delete(&url)?;
        assert!(f.save()?);
        assert!(!f.save()?);
        let f = JsonFile::open(&f.path, false)?;
        assert!(f.src.contains("// keep this comment") && f.src.contains("// provider comment"));
        assert_eq!(f.get(&["model"]), Some(json!("mygw/m2")));
        assert_eq!(f.get(&headers), Some(json!({"a":1})));
        assert_eq!(f.get(&url), None);
        Ok(())
    }

    #[test]
    fn trailing_commas() -> Result<()> {
        let (_dir, mut f) = fixture("{\n  \"a\": 1,\n}\n");
        f.set(&["b"], json!(2))?;
        assert_eq!(serde_json::from_str::<Value>(&f.src)?, json!({"a":1,"b":2}));
        f.set(&["c"], json!({"nested":true}))?;
        f.save()?;
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(f.path)?)?,
            json!({"a":1,"b":2,"c":{"nested":true}})
        );
        Ok(())
    }

    #[test]
    fn blank_files() -> Result<()> {
        for (src, key, v) in [("", "k", json!("v")), ("// nothing yet\n", "n", json!(1))] {
            let (_dir, mut f) = fixture(src);
            assert_eq!(f.get(&["anything"]), None);
            f.set(&[key], v.clone())?;
            f.save()?;
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(f.path)?)?,
                json!({(key):v})
            );
        }
        Ok(())
    }

    #[test]
    fn read_commented_values() -> Result<()> {
        let (_dir, mut f) = fixture(
            "{\n  \"provider\": {\n    // note\n    \"mygw\": {\n      // inner\n      \"a\": 1\n    }\n  },\n  \"list\": [\n    // first\n    {\"id\": \"x\"}\n  ]\n}\n",
        );
        assert_eq!(f.get(&["provider", "mygw"]), Some(json!({"a":1})));
        assert_eq!(f.get(&["list"]), Some(json!([{"id":"x"}])));
        f.set(&["provider", "mygw", "b"], json!("y"))?;
        assert_eq!(f.get(&["provider", "mygw"]), Some(json!({"a":1,"b":"y"})));
        f.save()?;
        assert!(fs::read_to_string(f.path)?.contains("// inner"));
        Ok(())
    }

    #[test]
    fn crlf_bom() -> Result<()> {
        let (_dir, mut f) = fixture("\u{feff}{\r\n  \"a\": 1\r\n}\r\n");
        f.set(&["b"], json!(2))?;
        f.save()?;
        assert_eq!(
            fs::read_to_string(f.path)?,
            "\u{feff}{\r\n  \"a\": 1,\r\n  \"b\": 2\r\n}\r\n"
        );
        Ok(())
    }

    #[test]
    fn create_and_extend_arrays() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("missing.json");
        assert!(JsonFile::open(&path, false).is_err());
        let mut f = JsonFile::open(&path, true)?;
        f.set(&["config", "items"], json!([]))?;
        f.set(&["config", "items", "2", "name"], json!("é"))?;
        f.set(&["config", "items", "4"], json!(42))?;
        f.delete(&["config", "items", "1"])?;
        assert_eq!(
            f.get(&["config", "items"]),
            Some(json!([{},{"name":"é"},null,42]))
        );
        f.save()?;
        Ok(())
    }
}
