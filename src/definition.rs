use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{fs, path::Path};

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub context: i64,
    pub output: i64,
    pub reasoning: bool,
    pub efforts: Vec<String>,
    pub default_effort: String,
    pub apis: Vec<String>,
    pub images: bool,
    pub input_modalities: Vec<String>,
}

#[derive(Clone)]
pub struct Definition {
    pub provider: String,
    pub api_key: String,
    pub v1: String,
    pub root: String,
    pub models: Vec<Model>,
    pub agents: Map<String, Value>,
}

#[derive(Default, Deserialize)]
struct Gateway {
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    base_url: String,
}
#[derive(Deserialize)]
struct Raw {
    provider: Option<Provider>,
    #[serde(default)]
    gateway: Gateway,
    #[serde(default)]
    models: Vec<Model>,
    #[serde(default)]
    agents: Map<String, Value>,
}
#[derive(Deserialize)]
struct Provider {
    id: Option<String>,
}

pub fn model_id(choice: &Value) -> Option<&str> {
    choice.as_str().or_else(|| choice.get("model")?.as_str())
}

pub fn load(path: &Path) -> Result<Definition> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let text = text.trim_start_matches('\u{feff}');
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Do not echo source snippets on parse errors: definitions contain API keys.
    let raw: Raw = match ext.as_str() {
        "toml" => toml::from_str(text).map_err(|_| anyhow::anyhow!("invalid TOML definition"))?,
        "json" => serde_json::from_str(text).map_err(|e| {
            anyhow::anyhow!("invalid JSON definition at {}:{}", e.line(), e.column())
        })?,
        "yaml" | "yml" => {
            serde_yaml::from_str(text).map_err(|_| anyhow::anyhow!("invalid YAML definition"))?
        }
        _ => bail!("unsupported definition format: {ext} (use .toml, .yaml or .json)"),
    };
    let base = raw.gateway.base_url.trim_end_matches('/');
    ensure!(!base.is_empty(), "definition needs gateway.base_url");
    ensure!(
        !raw.models.is_empty(),
        "definition has no [[models]] entries"
    );
    ensure!(
        raw.models.iter().all(|m| !m.id.is_empty()),
        "each model needs an id"
    );
    for (agent, choice) in &raw.agents {
        let id = model_id(choice)
            .filter(|s| !s.is_empty())
            .with_context(|| format!("[agents].{agent} needs a model id"))?;
        ensure!(
            raw.models.iter().any(|m| m.id == id),
            "[agents].{agent}: model {id:?} is not in [[models]]"
        );
    }
    let root = base.strip_suffix("/v1").unwrap_or(base).to_owned();
    Ok(Definition {
        v1: format!("{root}/v1"),
        root,
        api_key: raw.gateway.api_key,
        provider: raw
            .provider
            .and_then(|p| p.id)
            .unwrap_or_else(|| "sarai".into()),
        models: raw.models,
        agents: raw.agents,
    })
}
