use std::{env, fs, io::ErrorKind, path::Path};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::{Context, Definition, Model, write_atomic};

const CATALOG: &str = "sarai-models.json";
const PROMPT: &str = include_str!("../assets/codex_prompt.md");
const LEVELS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
const KEYS: [&str; 4] = [
    "model_provider",
    "model",
    "model_catalog_json",
    "model_reasoning_effort",
];

struct Config {
    original: Vec<u8>,
    doc: DocumentMut,
    bom: bool,
    crlf: bool,
}

impl Config {
    fn parse(original: Vec<u8>) -> Result<Self> {
        let input = std::str::from_utf8(&original)?;
        let bom = input.starts_with('\u{feff}');
        let crlf = input.contains("\r\n");
        let doc = input
            .strip_prefix('\u{feff}')
            .unwrap_or(input)
            .replace("\r\n", "\n")
            .parse()?;
        Ok(Self {
            original,
            doc,
            bom,
            crlf,
        })
    }

    fn load(path: &Path) -> Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        // The parser's diagnostic includes source lines, potentially with tokens.
        Self::parse(bytes).map_err(|_| anyhow!("invalid TOML in {}", path.display()))
    }

    fn render(&self) -> Vec<u8> {
        let mut text = self.doc.to_string();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        if self.bom {
            text.insert(0, '\u{feff}');
        }
        text.into_bytes()
    }

    fn save(&self, path: &Path) -> Result<bool> {
        let bytes = self.render();
        if self.original == bytes {
            return Ok(false);
        }
        write_atomic(path, &bytes)?;
        Ok(true)
    }

    fn provider(&mut self, id: &str, value: &Value) -> Result<()> {
        let root = self.doc.as_table_mut();
        if !root.contains_key("model_providers") {
            if value.is_null() {
                return Ok(());
            }
            let mut parent = Table::new();
            parent.set_implicit(true);
            root.insert("model_providers", Item::Table(parent));
        }
        let parent = root
            .get_mut("model_providers")
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| anyhow!("model_providers must be a table"))?;
        set(parent, id, value)
    }
}

// Edit matching tables in place: this retains key spelling, header placement,
// inline comments, and inline-table style while replacing the *whole* value.
fn set(table: &mut dyn TableLike, key: &str, new: &Value) -> Result<()> {
    if new.is_null() {
        table.remove(key);
    } else if let Some(fields) = new.as_object() {
        if !table.get(key).is_some_and(Item::is_table_like) {
            table.insert(key, Item::Table(Table::new()));
        }
        let dest = table
            .get_mut(key)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| anyhow!("unable to set table {key}"))?;
        let obsolete: Vec<_> = dest
            .iter()
            .filter(|(k, _)| !fields.contains_key(*k))
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in obsolete {
            dest.remove(&k);
        }
        for (k, v) in fields {
            set(dest, k, v)?;
        }
    } else {
        let mut item = if let Some(s) = new.as_str() {
            if table.get(key).and_then(Item::as_str) == Some(s) {
                return Ok(());
            }
            toml_edit::value(s)
        } else {
            // Use TOML's serializer for arbitrary stashed numbers and arrays.
            let text = toml::to_string(&json!({"v": new}))?;
            text.parse::<DocumentMut>()?
                .as_table_mut()
                .remove("v")
                .ok_or_else(|| anyhow!("unable to encode {key}"))?
        };
        if let Some(old) = table.get_mut(key) {
            if let (Some(a), Some(b)) = (old.as_value(), item.as_value_mut()) {
                *b.decor_mut() = a.decor().clone();
            }
            *old = item;
        } else {
            table.insert(key, item);
        }
    }
    Ok(())
}

fn efforts(m: &Model) -> Vec<&'static str> {
    LEVELS
        .into_iter()
        .filter(|e| m.efforts.iter().any(|s| s == *e))
        .collect()
}

fn default_effort<'a>(m: &'a Model, levels: &[&'static str]) -> Option<&'a str> {
    if levels.contains(&m.default_effort.as_str()) {
        Some(&m.default_effort)
    } else {
        levels.first().copied()
    }
}

fn entry(defn: &Definition, m: &Model, priority: usize) -> Value {
    let levels = efforts(m);
    let name = if m.name.is_empty() { &m.id } else { &m.name };
    json!({
        "slug": m.id, "display_name": name, "description": format!("{name} via {}", defn.provider),
        "base_instructions": PROMPT.replace("\r\n", "\n"),
        "default_reasoning_level": default_effort(m, &levels),
        "supported_reasoning_levels": levels.iter().map(|e| json!({"effort": e, "description": e})).collect::<Vec<_>>(),
        "shell_type": "unified_exec", "visibility": "list", "supported_in_api": true, "priority": priority,
        "support_verbosity": false, "apply_patch_tool_type": "freeform",
        "truncation_policy": {"mode": "tokens", "limit": 10000}, "experimental_supported_tools": [],
        "input_modalities": if m.images { vec!["text", "image"] } else { vec!["text"] },
        "context_window": (m.context != 0).then_some(m.context), "effective_context_window_percent": 95,
        "service_tiers": [], "supports_search_tool": true, "supports_parallel_tool_calls": true,
        "supports_reasoning_summaries": !levels.is_empty(),
    })
}

fn kept_catalog(bytes: &[u8], defn: &Definition) -> Result<Vec<Value>> {
    let mut old: Value =
        serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes))?;
    if old.is_object() {
        old = old["models"].take();
    }
    let Value::Array(mut entries) = old else {
        return Ok(Vec::new());
    };
    entries.retain(|e| {
        e.is_object()
            && !defn
                .models
                .iter()
                .any(|m| e["slug"].as_str() == Some(m.id.as_str()))
    });
    Ok(entries)
}

fn catalog_bytes(entries: Vec<Value>) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(&json!({ "models": entries }))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn run(ctx: &mut Context, restore: bool) -> Result<()> {
    let dir = env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| crate::expand(&s))
        .unwrap_or_else(|| crate::home().join(".codex"));
    let config = dir.join("config.toml");
    let catalog = dir.join(CATALOG);
    let defn = ctx.defn;
    let provider = defn.provider.as_str();
    if restore {
        if !ctx.ready(&dir) {
            return Ok(());
        }
        if config.exists() {
            let mut f = Config::load(&config)?;
            if let Some(old) = ctx.get("provider_table") {
                f.provider(provider, &old)?;
            }
            for key in KEYS {
                if let Some(old) = ctx.get(key) {
                    set(f.doc.as_table_mut(), key, &old)?;
                }
            }
            if f.save(&config)? {
                ctx.note(&config, "restored", "");
            }
        }
        if catalog.exists() {
            if ctx.get("catalog_absent") == Some(Value::Bool(true)) {
                fs::remove_file(&catalog)?;
                ctx.note(&catalog, "removed", "");
            } else if let Ok(kept) = kept_catalog(&fs::read(&catalog)?, defn) {
                write_atomic(&catalog, &catalog_bytes(kept)?)?;
                ctx.note(&catalog, "restored", "");
            }
        }
        return ctx.forget(&[
            "applied",
            "provider_table",
            "model_provider",
            "model",
            "model_catalog_json",
            "model_reasoning_effort",
            "catalog_absent",
        ]);
    }

    let m = ctx.model().clone();
    let effort = default_effort(&m, &efforts(&m));
    let detail = format!(
        "[model_providers.{provider}], model_provider, model, model_catalog_json; {}",
        effort
            .map(|s| format!("model_reasoning_effort = {s}"))
            .unwrap_or_else(|| "clear model_reasoning_effort".into())
    );
    let count = format!("{} entries", defn.models.len());
    if ctx.dry_run {
        ctx.note(&config, "would set", &detail);
        ctx.note(&catalog, "would write", &count);
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    let mut f = Config::load(&config)?;
    let original: toml::Table = toml::from_str(&f.doc.to_string())?;
    let old_provider = original
        .get("model_providers")
        .and_then(|v| v.get(provider));
    ctx.mark()?;
    ctx.remember("provider_table", serde_json::to_value(old_provider)?)?;
    for key in KEYS {
        ctx.remember(key, serde_json::to_value(original.get(key))?)?;
    }
    f.provider(
        provider,
        &json!({
            "name": provider, "base_url": defn.v1, "wire_api": "responses",
            "experimental_bearer_token": defn.api_key,
            "http_headers": { "x-openai-actor-authorization": provider }
        }),
    )?;
    for (k, v) in
        KEYS.into_iter()
            .zip([json!(provider), json!(m.id), json!(CATALOG), json!(effort)])
    {
        set(f.doc.as_table_mut(), k, &v)?;
    }
    if f.save(&config)? {
        ctx.note(&config, "set", &detail);
    }
    let old = match fs::read(&catalog) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            ctx.remember("catalog_absent", json!(true))?;
            Vec::new()
        }
        Err(e) => return Err(e.into()),
    };
    let mut entries = kept_catalog(&old, defn).unwrap_or_default();
    entries.extend(
        defn.models
            .iter()
            .enumerate()
            .map(|(i, m)| entry(defn, m, i + 1)),
    );
    let bytes = catalog_bytes(entries)?;
    if old.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&old) != bytes.as_slice() {
        write_atomic(&catalog, &bytes)?;
        ctx.note(&catalog, "wrote", &count);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_edit_preserves_surrounding_bytes() -> Result<()> {
        let original =
            "\u{feff}# setup\r\n'model'  =  'mine' # active\r\n\r\n[user]\r\nx = 0x10\r\n";
        let mut f = Config::parse(original.as_bytes().to_vec())?;
        set(f.doc.as_table_mut(), "model", &json!("new"))?;
        assert_eq!(f.render(), original.replace("'mine'", "\"new\"").as_bytes());
        Ok(())
    }

    #[test]
    fn preserves_neighboring_providers_and_repeated_edits() -> Result<()> {
        let other = "[model_providers.other] # untouched\nurl  = 'other'\n";
        let mut f = Config::parse(
            format!(
                "{other}\n[model_providers.'a.b'] # chosen\nname = 'old' # keep\nobsolete = 1\n"
            )
            .into_bytes(),
        )?;
        let provider = json!({"name":"new","http_headers":{"x-openai-actor-authorization":"a.b"}});
        f.provider("a.b", &provider)?;
        let text = f.doc.to_string();
        assert!(text.starts_with(other));
        assert!(text.contains("# chosen") && text.contains("# keep") && !text.contains("obsolete"));
        let mut again = Config::parse(f.render())?;
        again.provider("a.b", &provider)?;
        assert_eq!(again.render(), f.render());
        Ok(())
    }
}
