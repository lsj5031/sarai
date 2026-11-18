use super::{Context, Definition, Model, expand, home, jsonfile::JsonFile, merge};
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::{
    env,
    path::{Path, PathBuf},
};

const LEAVES: &[&str] = &[
    "defaultProvider",
    "defaultModel",
    "defaultThinkingLevel",
    "enabledModels",
];
fn opt<'a>(ctx: &'a Context, k: &str) -> Option<&'a Value> {
    ctx.defn.agents.get(ctx.agent)?.get(k)
}
fn envpath(k: &str) -> Option<PathBuf> {
    env::var(k)
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| expand(&s))
}
fn name(m: &Model) -> &str {
    if m.name.is_empty() { &m.id } else { &m.name }
}
fn speaks(m: &Model, api: &str) -> bool {
    m.apis.iter().any(|a| a == api)
}
fn set(ctx: &mut Context, f: &mut JsonFile, key: &str, path: &[&str], v: Value) -> Result<()> {
    ctx.remember(key, f.get(path).unwrap_or(Value::Null))?;
    f.set(path, v)
}
fn undo(ctx: &Context, f: &mut JsonFile, key: &str, path: &[&str]) -> Result<()> {
    match ctx.get(key) {
        None => Ok(()),
        Some(Value::Null) => f.delete(path),
        Some(v) => f.set(path, v),
    }
}
fn save(ctx: &mut Context, f: &JsonFile, p: &Path, restore: bool, detail: &str) -> Result<()> {
    if f.save()? {
        ctx.note(p, if restore { "restored" } else { "set" }, detail);
    }
    Ok(())
}
fn pi_model(d: &Definition, m: &Model) -> Value {
    let mut e = json!({"id":m.id,"name":name(m),"reasoning":m.reasoning||!m.efforts.is_empty()});
    if speaks(m, "anthropic") {
        e["api"] = json!("anthropic-messages");
        e["baseUrl"] = json!(d.root);
    } else if speaks(m, "responses") {
        e["api"] = json!("openai-responses");
    }
    if m.context != 0 {
        e["contextWindow"] = json!(m.context);
    }
    if m.output != 0 {
        e["maxTokens"] = json!(m.output);
    }
    if m.images {
        e["input"] = json!(["text", "image"]);
    } else if !m.input_modalities.is_empty() {
        e["input"] = json!(m.input_modalities);
    }
    if !m.efforts.is_empty() {
        e["thinkingLevelMap"] =
            Value::Object(m.efforts.iter().map(|s| (s.clone(), json!(s))).collect());
    }
    e
}
fn wanted(reference: &str, scope: &[Value]) -> bool {
    let full = reference.to_lowercase();
    let short = full.split_once('/').map_or(&*full, |v| v.1);
    scope.iter().filter_map(Value::as_str).any(|p| {
        let p = p.trim().to_lowercase();
        p == full
            || p == short
            || glob::Pattern::new(&p).is_ok_and(|g| g.matches(&full) || g.matches(short))
    })
}
fn pi(ctx: &mut Context, restore: bool) -> Result<()> {
    let dir = expand(
        opt(ctx, "directory")
            .and_then(Value::as_str)
            .unwrap_or(match ctx.agent {
                "omo" => "~/.omo/agent",
                "mu" => "~/.mu/agent",
                "stepcode" => "~/.stepcode",
                "prime-agent" => "~/.prime/agent",
                _ => "~/.pi/agent",
            }),
    );
    let (settings, models) = (dir.join("settings.json"), dir.join("models.json"));
    let d = ctx.defn;
    let path = ["providers", &d.provider];
    let detail = path.join(".");
    if restore {
        if !ctx.ready(&settings) {
            return Ok(());
        }
        if models.exists() {
            let mut f = JsonFile::open(&models, false)?;
            undo(ctx, &mut f, "models_block", &path)?;
            save(ctx, &f, &models, true, "")?;
        }
        if settings.exists() {
            let mut f = JsonFile::open(&settings, false)?;
            for &k in LEAVES {
                undo(ctx, &mut f, k, &[k])?;
            }
            save(ctx, &f, &settings, true, "")?;
        }
        return ctx.forget(&[LEAVES, &["models_block", "applied"]].concat());
    }
    let mut mf = JsonFile::open(&models, true)?;
    let mut sf = JsonFile::open(&settings, true)?;
    if ctx.dry_run {
        ctx.note(&models, "would set", &detail);
        ctx.note(&settings, "would set", "defaultProvider/defaultModel");
        return Ok(());
    }
    let model = ctx.model().id.clone();
    let reference = format!("{}/{}", d.provider, model);
    let block = json!({"name":d.provider,"baseUrl":d.v1,"api":"openai-completions","apiKey":d.api_key,"compat":{"sendSessionAffinityHeaders":true},"models":d.models.iter().map(|m|pi_model(d,m)).collect::<Vec<_>>()});
    ctx.mark()?;
    let block = merge(block, mf.get(&path), &["models"]);
    set(ctx, &mut mf, "models_block", &path, block)?;
    save(ctx, &mf, &models, false, &detail)?;
    for &k in LEAVES {
        ctx.remember(k, sf.get(&[k]).unwrap_or(Value::Null))?;
    }
    sf.set(&["defaultProvider"], json!(d.provider))?;
    sf.set(&["defaultModel"], json!(model))?;
    if let Some(Value::Array(mut scope)) = sf.get(&["enabledModels"])
        && !wanted(&reference, &scope)
    {
        scope.push(json!(reference));
        sf.set(&["enabledModels"], Value::Array(scope))?;
    }
    if let Some(e) = opt(ctx, "effort")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
    {
        sf.set(&["defaultThinkingLevel"], json!(e))?;
    }
    save(ctx, &sf, &settings, false, "defaultProvider/defaultModel")
}
fn droid_entry(d: &Definition, m: &Model) -> Value {
    let provider = if speaks(m, "anthropic") {
        "anthropic"
    } else if speaks(m, "responses") {
        "openai"
    } else {
        "generic-chat-completion-api"
    };
    let mut e = json!({"model":m.id,"id":format!("custom:{}/{}",d.provider,m.id),"displayName":name(m),"baseUrl":if provider=="anthropic"{&d.root}else{&d.v1},"apiKey":d.api_key,"provider":provider});
    if m.context != 0 {
        e["maxContextLimit"] = json!(m.context);
    }
    if m.output != 0 {
        e["maxOutputTokens"] = json!(m.output);
    }
    if !m.efforts.is_empty() {
        let order = ["medium", "high", "low", "xhigh", "max", "minimal"];
        let supported = |s: &str| m.efforts.iter().any(|x| x == s);
        e["reasoningEffort"] = json!(if order.contains(&m.default_effort.as_str())
            && supported(&m.default_effort)
        {
            &m.default_effort
        } else {
            order.into_iter().find(|s| supported(s)).unwrap_or("medium")
        });
    }
    e["noImageSupport"] = json!(!m.images);
    e
}
fn droid(ctx: &mut Context, restore: bool) -> Result<()> {
    let p = envpath("FACTORY_HOME_OVERRIDE")
        .unwrap_or_else(home)
        .join(".factory/settings.json");
    let model = ["sessionDefaultSettings", "model"];
    if restore {
        if !ctx.ready(&p) {
            return Ok(());
        }
        if p.exists() {
            let mut f = JsonFile::open(&p, false)?;
            undo(ctx, &mut f, "customModels", &["customModels"])?;
            undo(ctx, &mut f, "model", &model)?;
            save(ctx, &f, &p, true, "")?;
        }
        return ctx.forget(&["customModels", "model", "applied"]);
    }
    let d = ctx.defn;
    let detail = format!(
        "customModels ({}), sessionDefaultSettings.model",
        d.models.len()
    );
    if ctx.dry_run {
        ctx.note(&p, "would set", &detail);
        return Ok(());
    }
    let mut f = JsonFile::open(&p, true)?;
    ctx.mark()?;
    let prefix = format!("custom:{}/", d.provider);
    let mut entries = match f.get(&["customModels"]) {
        Some(Value::Array(list)) => list,
        _ => vec![],
    };
    entries.retain(|e| !e["id"].as_str().is_some_and(|s| s.starts_with(&prefix)));
    entries.extend(d.models.iter().map(|m| droid_entry(d, m)));
    set(
        ctx,
        &mut f,
        "customModels",
        &["customModels"],
        json!(entries),
    )?;
    let id = format!("{prefix}{}", ctx.model().id);
    set(ctx, &mut f, "model", &model, json!(id))?;
    save(ctx, &f, &p, false, &detail)
}
fn oc_model(m: &Model, two: bool) -> Value {
    let mut e = json!({"name":name(m)});
    if m.images {
        if two {
            e["capabilities"] = json!({"tools":true,"input":["text","image"],"output":["text"]});
        } else {
            e["modalities"] = json!({"input":["text","image"],"output":["text"]});
        }
    }
    if m.context != 0 || m.output != 0 {
        e["limit"] = json!({"context":m.context,"output":m.output});
    }
    if !two && (m.reasoning || !m.efforts.is_empty()) {
        e["reasoning"] = json!(true);
    }
    if !m.efforts.is_empty() {
        let mut levels: Vec<&str> = ["minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .filter(|s| m.efforts.iter().any(|e| e == s))
            .collect();
        if levels.is_empty() {
            levels.extend(m.efforts.iter().map(String::as_str));
        }
        e["variants"] = if two {
            Value::Array(
                levels
                    .iter()
                    .map(|s| {
                        let mut v = json!({"id":s,"settings":{"reasoningEffort":s}});
                        if s.ends_with("-fast") {
                            v["body"] = json!({});
                        }
                        v
                    })
                    .collect(),
            )
        } else {
            Value::Object(
                levels
                    .iter()
                    .map(|s| (s.to_string(), json!({"reasoningEffort":s})))
                    .collect(),
            )
        };
    }
    e
}
fn opencode(ctx: &mut Context, restore: bool) -> Result<()> {
    let a = ctx.agent;
    let base = opt(ctx, "directory")
        .and_then(Value::as_str)
        .map(expand)
        .or_else(|| envpath("XDG_CONFIG_HOME"))
        .unwrap_or_else(|| home().join(".config"))
        .join(a);
    let dir = if a == "opencode" {
        envpath("OPENCODE_CONFIG_DIR").unwrap_or(base)
    } else {
        base
    };
    let p = [
        format!("{a}.jsonc"),
        format!("{a}.json"),
        "config.json".into(),
    ]
    .iter()
    .map(|n| dir.join(n))
    .find(|p| p.exists())
    .unwrap_or_else(|| dir.join(format!("{a}.json")));
    let d = ctx.defn;
    if restore && !ctx.ready(&p) {
        return Ok(());
    }
    if restore && !p.exists() {
        return ctx.forget(&[
            "applied",
            "section",
            "model",
            "small_model",
            "provider_block",
        ]);
    }
    let mut f = JsonFile::open(&p, !restore)?;
    let two = f.get(&["providers"]).is_some()
        || opt(ctx, "opencode_version").is_some_and(|v| v == 2 || v == "2" || v == "v2");
    let section = if two { "providers" } else { "provider" };
    if restore {
        let old = ctx.get("section");
        let section = old.as_ref().and_then(Value::as_str).unwrap_or(section);
        undo(ctx, &mut f, "provider_block", &[section, &d.provider])?;
        for k in ["model", "small_model"] {
            undo(ctx, &mut f, k, &[k])?;
        }
        save(ctx, &f, &p, true, "")?;
        return ctx.forget(&[
            "applied",
            "section",
            "model",
            "small_model",
            "provider_block",
        ]);
    }
    let key = [section, &d.provider];
    let detail = key.join(".");
    if ctx.dry_run {
        ctx.note(&p, "would set", &detail);
        return Ok(());
    }
    let mut block = if two {
        json!({"package":"aisdk:@ai-sdk/openai-compatible","name":d.provider,"settings":{"baseURL":d.v1,"apiKey":d.api_key}})
    } else {
        json!({"npm":"@ai-sdk/openai-compatible","name":d.provider,"options":{"baseURL":d.v1,"apiKey":d.api_key,"setCacheKey":true}})
    };
    block["models"] = Value::Object(
        d.models
            .iter()
            .map(|m| (m.id.clone(), oc_model(m, two)))
            .collect::<Map<_, _>>(),
    );
    ctx.mark()?;
    ctx.remember("section", json!(section))?;
    let block = merge(block, f.get(&key), &["models"]);
    set(ctx, &mut f, "provider_block", &key, block)?;
    let reference = json!(format!("{}/{}", d.provider, ctx.model().id));
    set(ctx, &mut f, "model", &["model"], reference.clone())?;
    ctx.remember(
        "small_model",
        f.get(&["small_model"]).unwrap_or(Value::Null),
    )?;
    if !opt(ctx, "small")
        .and_then(Value::as_str)
        .unwrap_or("")
        .eq_ignore_ascii_case("none")
    {
        f.set(&["small_model"], reference)?;
    }
    save(ctx, &f, &p, false, &format!("{detail}, model, small_model"))
}
pub fn run(ctx: &mut Context, restore: bool) -> Result<()> {
    match ctx.agent {
        "droid" => droid(ctx, restore),
        "opencode" | "mimocode" | "deveco" => opencode(ctx, restore),
        _ => pi(ctx, restore),
    }
}
