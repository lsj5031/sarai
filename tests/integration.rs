//! End-to-end tests for the compiled `sarai` binary.
//!
//! Each test runs the CLI in its own temporary home, so a real agent
//! installation is never touched.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DEF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/defs/example.toml");
const MODEL_REF: &str = "mygw/deepseek/deepseek-v4-pro";

struct Sandbox {
    home: PathBuf,
    state: PathBuf,
    _dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let state = home.join(".state");
        Self {
            home,
            state,
            _dir: dir,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sarai"))
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("CODEX_HOME", self.home.join(".codex"))
            .env("FACTORY_HOME_OVERRIDE", &self.home)
            .env_remove("OPENCODE_CONFIG_DIR")
            .output()
            .expect("run sarai")
    }

    fn run_ok(&self, args: &[&str]) -> Output {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "sarai {args:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
}

fn seeded() -> Sandbox {
    let s = Sandbox::new();

    let pi = s.path(".pi/agent");
    fs::create_dir_all(&pi).unwrap();
    write(
        &pi.join("settings.json"),
        "{\n  // my settings\n  \"defaultProvider\": \"old\",\n  \"defaultModel\": \"m\",\n  \"enabledModels\": [\"old/*\"]\n}\n",
    );
    write(
        &pi.join("models.json"),
        "{\n  // my models\n  \"providers\": {\"other\": {\"name\": \"other\"}}\n}\n",
    );

    let factory = s.path(".factory");
    fs::create_dir_all(&factory).unwrap();
    write(
        &factory.join("settings.json"),
        "{\n  \"customModels\": [{\"id\": \"custom:mine\", \"model\": \"theirs\"}],\n  \"sessionDefaultSettings\": {\"model\": \"custom:mine\"}\n}\n",
    );

    // CRLF on purpose: applying must not rewrite every line ending.
    let codex = s.path(".codex");
    fs::create_dir_all(&codex).unwrap();
    write(
        &codex.join("config.toml"),
        "# my config\r\nmodel = \"user-model\"\r\nmodel_provider = \"openai\"\r\n\r\n[model_providers.mygw]\r\nname = \"old-name\"\r\nbase_url = \"http://old/v1\"\r\nwire_api = \"chat\"\r\n",
    );

    let opencode = s.path(".config/opencode");
    fs::create_dir_all(&opencode).unwrap();
    write(
        &opencode.join("opencode.json"),
        "{\n  // my opencode\n  \"model\": \"old/m\",\n  \"small_model\": \"old/s\"\n}\n",
    );
    s
}

fn files(s: &Sandbox) -> [PathBuf; 5] {
    [
        s.path(".pi/agent/settings.json"),
        s.path(".pi/agent/models.json"),
        s.path(".factory/settings.json"),
        s.path(".codex/config.toml"),
        s.path(".config/opencode/opencode.json"),
    ]
}

fn write(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn read_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn has(haystack: &str, needle: &str, what: &str) {
    assert!(
        haystack.contains(needle),
        "{what}: expected {needle:?} in\n{haystack}"
    );
}

#[test]
fn apply_is_surgical_and_idempotent() {
    let s = seeded();
    s.run_ok(&["-d", DEF]);

    let settings = read(&s.path(".pi/agent/settings.json"));
    has(&settings, "\"defaultProvider\": \"mygw\"", "pi settings");
    has(&settings, "// my settings", "pi settings");
    has(&settings, &format!("\"{MODEL_REF}\""), "pi settings");

    let models = read(&s.path(".pi/agent/models.json"));
    has(&models, "// my models", "pi models");
    has(&models, "\"other\"", "pi models");
    has(&models, "\"mygw\"", "pi models");

    let droid = read(&s.path(".factory/settings.json"));
    has(&droid, "custom:mine", "droid settings");
    has(&droid, &format!("custom:{MODEL_REF}"), "droid settings");
    has(
        &droid,
        &format!("\"sessionDefaultSettings\": {{\"model\": \"custom:{MODEL_REF}\"}}"),
        "droid settings",
    );

    let raw = read_bytes(&s.path(".codex/config.toml"));
    let codex = String::from_utf8_lossy(&raw);
    has(&codex, "# my config", "codex config");
    has(&codex, "model_provider = \"mygw\"", "codex config");
    has(&codex, "[model_providers.mygw]", "codex config");
    has(
        &codex,
        "model_reasoning_effort = \"medium\"",
        "codex config",
    );
    assert!(
        raw.iter()
            .enumerate()
            .all(|(i, b)| *b != b'\n' || (i > 0 && raw[i - 1] == b'\r')),
        "config gained bare LF line endings"
    );
    assert!(
        !codex.contains("model_catalog_json"),
        "codex config still references a catalog"
    );
    assert!(
        !s.path(".codex/sarai-models.json").exists(),
        "codex catalog should not be written"
    );

    let opencode = read(&s.path(".config/opencode/opencode.json"));
    has(&opencode, "// my opencode", "opencode config");
    has(
        &opencode,
        &format!("\"model\": \"{MODEL_REF}\""),
        "opencode config",
    );
    has(&opencode, "\"mygw\"", "opencode config");

    let files = files(&s);
    let before: Vec<Vec<u8>> = files.iter().map(|p| read_bytes(p)).collect();
    s.run_ok(&["-d", DEF]);
    for (path, bytes) in files.iter().zip(&before) {
        assert_eq!(
            &read_bytes(path),
            bytes,
            "second apply changed {}",
            path.display()
        );
    }
}

#[test]
fn restore_puts_originals_back() {
    let s = seeded();
    s.run_ok(&["-d", DEF]);
    s.run_ok(&["-d", DEF, "--restore"]);

    let settings = read(&s.path(".pi/agent/settings.json"));
    has(&settings, "\"defaultProvider\": \"old\"", "pi settings");
    has(&settings, "\"defaultModel\": \"m\"", "pi settings");
    has(&settings, "\"enabledModels\": [\"old/*\"]", "pi settings");

    let droid = read(&s.path(".factory/settings.json"));
    assert!(
        !droid.contains("custom:mygw/"),
        "droid kept the injected model:\n{droid}"
    );
    has(&droid, "custom:mine", "droid settings");
    has(&droid, "\"model\": \"custom:mine\"", "droid settings");

    let codex = String::from_utf8_lossy(&read_bytes(&s.path(".codex/config.toml"))).into_owned();
    has(&codex, "model = \"user-model\"", "codex config");
    has(&codex, "model_provider = \"openai\"", "codex config");
    has(&codex, "name = \"old-name\"", "codex config");
    has(&codex, "wire_api = \"chat\"", "codex config");
    assert!(
        !codex.contains("model_reasoning_effort"),
        "effort was not cleared:\n{codex}"
    );
    let opencode = read(&s.path(".config/opencode/opencode.json"));
    has(&opencode, "\"model\": \"old/m\"", "opencode config");

    // A second restore has nothing stashed and must be a no-op, not a delete.
    let restored: Vec<(PathBuf, Vec<u8>)> = files(&s)
        .into_iter()
        .filter(|p| p.exists())
        .map(|p| {
            let bytes = read_bytes(&p);
            (p, bytes)
        })
        .collect();
    s.run_ok(&["-d", DEF, "--restore"]);
    for (path, bytes) in restored {
        assert_eq!(
            read_bytes(&path),
            bytes,
            "second restore changed {}",
            path.display()
        );
    }
}

#[test]
fn switching_models_updates_effort_and_restores_it() {
    let s = Sandbox::new();
    let codex_dir = s.path(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let config = codex_dir.join("config.toml");
    write(&config, "model_reasoning_effort = \"high\"\r\n");

    let custom = s.path("switch.json");
    let mut definition = json!({
        "gateway": {"base_url": "http://localhost/v1"},
        "models": [
            {"id": "reasoner", "efforts": ["medium", "low"], "default_effort": "unknown"},
            {"id": "plain"}
        ],
        "agents": {"codex": "reasoner"}
    });
    write(&custom, &definition.to_string());

    let def = custom.to_str().unwrap();
    s.run_ok(&["-d", def]);
    let config_text = String::from_utf8_lossy(&read_bytes(&config)).into_owned();
    has(
        &config_text,
        "model_reasoning_effort = \"low\"",
        "codex effort",
    );

    // Switching to a model with no levels must clear the injected effort.
    definition["agents"]["codex"] = json!("plain");
    write(&custom, &definition.to_string());
    s.run_ok(&["-d", def]);
    let config_text = String::from_utf8_lossy(&read_bytes(&config)).into_owned();
    assert!(
        !config_text.contains("model_reasoning_effort"),
        "effort was not cleared:\n{config_text}"
    );

    s.run_ok(&["-d", def, "--restore"]);
    let config_text = String::from_utf8_lossy(&read_bytes(&config)).into_owned();
    has(
        &config_text,
        "model_reasoning_effort = \"high\"",
        "restored effort",
    );
}

#[test]
fn codex_legacy_catalog_is_removed() {
    let s = Sandbox::new();
    let codex = s.path(".codex");
    fs::create_dir_all(&codex).unwrap();
    write(
        &codex.join("config.toml"),
        "model_catalog_json = \"sarai-models.json\"\n",
    );
    write(&codex.join("sarai-models.json"), "{\"models\":[]}\n");

    s.run_ok(&["-d", DEF, "-a", "codex"]);

    let config = read(&codex.join("config.toml"));
    assert!(
        !config.contains("model_catalog_json"),
        "legacy catalog reference was kept:\n{config}"
    );
    assert!(
        !codex.join("sarai-models.json").exists(),
        "legacy catalog file was kept"
    );
}

#[test]
fn dry_run_writes_nothing() {
    let s = seeded();
    s.run_ok(&["-d", DEF]);

    let files = files(&s);
    let before: Vec<Vec<u8>> = files.iter().map(|p| read_bytes(p)).collect();
    s.run_ok(&["-d", DEF, "--dry-run"]);
    s.run_ok(&["-d", DEF, "--restore", "--dry-run"]);
    for (path, bytes) in files.iter().zip(&before) {
        assert_eq!(
            &read_bytes(path),
            bytes,
            "dry run changed {}",
            path.display()
        );
    }

    // A dry run must not create config directories either.
    let fresh = Sandbox::new();
    fresh.run_ok(&["-d", DEF, "-a", "pi,droid,codex,opencode", "--dry-run"]);
    for relative in [".pi", ".config", ".codex", ".factory"] {
        assert!(!fresh.path(relative).exists(), "dry run created {relative}");
    }
}

#[test]
fn a_model_typo_is_a_hard_error() {
    let s = Sandbox::new();
    let bad = s.path("bad.toml");
    write(
        &bad,
        "[provider]\nid = \"mygw\"\n\n[gateway]\nbase_url = \"http://127.0.0.1:1/v1\"\n\n[[models]]\nid = \"m/one\"\n\n[agents]\npi = \"m/typo\"\n",
    );

    let out = s.run(&["-d", bad.to_str().unwrap(), "--dry-run"]);
    assert!(!out.status.success(), "a model id typo was accepted");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("m/typo"), "stderr: {stderr}");
}
