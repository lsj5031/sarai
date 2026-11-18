mod codex;
mod definition;
mod json_agents;
mod jsonfile;

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use definition::{Definition, Model};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const AGENTS: &[&str] = &[
    "codex",
    "deveco",
    "droid",
    "mimocode",
    "mu",
    "omo",
    "opencode",
    "pi",
    "prime-agent",
    "stepcode",
];

#[derive(Parser)]
#[command(
    version,
    about = "Inject your own provider and models into coding agents' config files."
)]
struct Args {
    /// Definition file (.toml, .yaml or .json); defaults to defs/example.toml
    #[arg(short = 'd', long = "def")]
    definition: Option<PathBuf>,
    /// Comma-separated agents (default: all agents in the definition)
    #[arg(short = 'a', long)]
    agents: Option<String>,
    #[arg(long)]
    restore: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    no_stash: bool,
    #[arg(long)]
    list: bool,
    /// Original-value stash directory (default: .state beside the executable)
    #[arg(long)]
    state_dir: Option<PathBuf>,
}

fn home() -> PathBuf {
    std::env::home_dir().expect("cannot resolve home directory")
}
fn expand(path: &str) -> PathBuf {
    let mut out = String::new();
    let mut rest = path;
    while !rest.is_empty() {
        let var = if let Some(s) = rest.strip_prefix("${") {
            s.find('}').map(|i| (&s[..i], i + 3))
        } else if let Some(s) = rest.strip_prefix('$') {
            let n = s
                .bytes()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                .count();
            (n > 0).then(|| (&s[..n], n + 1))
        } else if cfg!(windows) && rest.starts_with('%') {
            rest[1..].find('%').map(|i| (&rest[1..i + 1], i + 2))
        } else {
            None
        };
        if let Some((name, n)) = var {
            out.push_str(&env::var(name).unwrap_or_else(|_| rest[..n].into()));
            rest = &rest[n..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    if out == "~" {
        home()
    } else if let Some(p) = out.strip_prefix("~/").or_else(|| out.strip_prefix("~\\")) {
        home().join(p)
    } else {
        out.into()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    if let Ok(meta) = path.metadata() {
        tmp.as_file().set_permissions(meta.permissions())?;
    }
    tmp.persist(path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}
fn merge(mut ours: Value, theirs: Option<Value>, whole: &[&str]) -> Value {
    if let (Some(a), Some(Value::Object(b))) = (ours.as_object_mut(), theirs) {
        for (k, v) in b {
            if let Some(o) = a.get_mut(&k) {
                if o.is_object() && v.is_object() && !whole.contains(&k.as_str()) {
                    *o = merge(o.take(), Some(v), &[]);
                }
            } else {
                a.insert(k, v);
            }
        }
    }
    ours
}

type State = BTreeMap<String, BTreeMap<String, Value>>;
struct Stash {
    path: PathBuf,
    data: State,
    enabled: bool,
}
impl Stash {
    fn open(path: PathBuf, enabled: bool) -> Result<Self> {
        let data = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("invalid stash; refusing to overwrite originals")?,
            Err(e) if e.kind() == ErrorKind::NotFound => State::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path,
            data,
            enabled,
        })
    }
    fn save(&self) -> Result<()> {
        let mut b = serde_json::to_vec_pretty(&self.data)?;
        b.push(b'\n');
        write_atomic(&self.path, &b)
    }
}
struct Context<'a> {
    defn: &'a Definition,
    agent: &'a str,
    dry_run: bool,
    stash: &'a mut Stash,
    changed: bool,
}
impl Context<'_> {
    fn model(&self) -> &Model {
        let id =
            definition::model_id(&self.defn.agents[self.agent]).expect("validated agent choice");
        self.defn
            .models
            .iter()
            .find(|m| m.id == id)
            .expect("validated model")
    }
    fn get(&self, key: &str) -> Option<Value> {
        self.stash.data.get(self.agent)?.get(key).cloned()
    }
    fn remember(&mut self, key: &str, v: Value) -> Result<()> {
        if self.stash.enabled && self.get(key).is_none() {
            self.stash
                .data
                .entry(self.agent.into())
                .or_default()
                .insert(key.into(), v);
            self.stash.save()?;
        }
        Ok(())
    }
    fn forget(&mut self, keys: &[&str]) -> Result<()> {
        if self.stash.enabled
            && let Some(e) = self.stash.data.get_mut(self.agent)
        {
            for k in keys {
                e.remove(*k);
            }
            if e.is_empty() {
                self.stash.data.remove(self.agent);
            }
            self.stash.save()?;
        }
        Ok(())
    }
    fn mark(&mut self) -> Result<()> {
        self.remember("applied", json!(true))
    }
    fn note(&mut self, path: &Path, action: &str, detail: &str) {
        let tail = if detail.is_empty() {
            String::new()
        } else {
            format!("  ({detail})")
        };
        println!("{}: {action} {}{tail}", self.agent, path.display());
        self.changed = true;
    }
    fn ready(&mut self, path: &Path) -> bool {
        if self.get("applied").is_none() {
            self.note(path, "restore skipped", "no stashed originals in .state");
            false
        } else if self.dry_run {
            self.note(path, "would restore", "");
            false
        } else {
            true
        }
    }
}
fn run(args: Args) -> Result<bool> {
    if args.list {
        println!("supported agents:");
        for id in AGENTS {
            println!("  {id}");
        }
        return Ok(true);
    }
    let exe = env::current_exe()?;
    let dir = exe.parent().context("executable has no parent")?;
    let default = Path::new("defs/example.toml");
    let path = args.definition.unwrap_or_else(|| {
        if default.exists() {
            default.into()
        } else {
            dir.join(default)
        }
    });
    let defn = definition::load(&path)?;
    let agents: Vec<&str> = match args.agents.as_deref() {
        Some(s) => s
            .split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .collect(),
        None => defn.agents.keys().map(String::as_str).collect(),
    };
    let mut stash = Stash::open(
        args.state_dir
            .unwrap_or_else(|| dir.join(".state"))
            .join("stash.json"),
        !args.no_stash && !args.dry_run,
    )?;
    let (mut ok, mut changed) = (true, false);
    for agent in agents {
        let mut ctx = Context {
            defn: &defn,
            agent,
            dry_run: args.dry_run,
            stash: &mut stash,
            changed: false,
        };
        let r = (|| {
            if !AGENTS.contains(&agent) {
                bail!("not supported (see --list)");
            }
            if !args.restore && !defn.agents.contains_key(agent) {
                bail!("no model configured in the definition's [agents]");
            }
            if agent == "codex" {
                codex::run(&mut ctx, args.restore)
            } else {
                json_agents::run(&mut ctx, args.restore)
            }
        })();
        if let Err(e) = r {
            eprintln!("error: {agent}: {e:#}");
            ok = false;
        }
        changed |= ctx.changed;
    }
    if ok && !changed {
        println!("nothing to do");
    }
    Ok(ok)
}
fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
