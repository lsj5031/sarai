import os
import tempfile
from pathlib import Path

from injector.__main__ import main
from injector.definition import load as load_definition

home = Path(tempfile.mkdtemp())
os.environ["USERPROFILE"] = str(home)
os.environ["HOME"] = str(home)
os.environ.pop("OPENCODE_CONFIG_DIR", None)
os.environ["XDG_CONFIG_HOME"] = str(home / ".config")

# Pi
pi = home / ".pi" / "agent"
pi.mkdir(parents=True)
(pi / "settings.json").write_text(
    '{\n  // my settings\n  "defaultProvider": "old",\n  "defaultModel": "m",\n'
    '  "enabledModels": ["old/*"]\n}\n', encoding="utf-8")
(pi / "models.json").write_text(
    '{\n  // my models\n  "providers": {"other": {"name": "other"}}\n}\n', encoding="utf-8")

# Droid
factory = home / ".factory"
factory.mkdir()
(factory / "settings.json").write_text(
    '{\n  "customModels": [{"id": "custom:mine", "model": "theirs"}],\n'
    '  "sessionDefaultSettings": {"model": "custom:mine"}\n}\n', encoding="utf-8")

# Codex, CRLF on purpose: applying must not rewrite every line ending.
codex = home / ".codex"
codex.mkdir()
(codex / "config.toml").write_bytes(
    b'# my config\r\nmodel = "user-model"\r\nmodel_provider = "openai"\r\n\r\n'
    b'[model_providers.mygw]\r\nname = "old-name"\r\nbase_url = "http://old/v1"\r\n'
    b'wire_api = "chat"\r\n')

# OpenCode
oc = home / ".config" / "opencode"
oc.mkdir(parents=True)
(oc / "opencode.json").write_text(
    '{\n  // my opencode\n  "model": "old/m",\n  "small_model": "old/s"\n}\n', encoding="utf-8")

rc = main(["-d", "defs/example.toml"])
assert rc == 0, rc

pi_settings = (pi / "settings.json").read_text(encoding="utf-8")
assert '"defaultProvider": "mygw"' in pi_settings
assert "// my settings" in pi_settings
assert '"mygw/deepseek/deepseek-v4-pro"' in pi_settings
pi_models = (pi / "models.json").read_text(encoding="utf-8")
assert "// my models" in pi_models and '"other"' in pi_models and '"mygw"' in pi_models

droid = (factory / "settings.json").read_text(encoding="utf-8")
assert "custom:mine" in droid and "custom:mygw/deepseek/deepseek-v4-pro" in droid
assert '"sessionDefaultSettings": {"model": "custom:mygw/deepseek/deepseek-v4-pro"}' in droid

codex_raw = (codex / "config.toml").read_bytes()
assert b"# my config" in codex_raw
assert b'model_provider = "mygw"' in codex_raw
assert b"[model_providers.mygw]" in codex_raw
assert b"\r\n" in codex_raw and b"\n" not in codex_raw.replace(b"\r\n", b"")
assert (codex / "magpie-models.json").exists()

oc_cfg = (oc / "opencode.json").read_text(encoding="utf-8")
assert "// my opencode" in oc_cfg
assert '"model": "mygw/deepseek/deepseek-v4-pro"' in oc_cfg
assert '"mygw"' in oc_cfg

# Idempotence: a second apply must not change bytes.
files = [pi / "settings.json", pi / "models.json", factory / "settings.json",
         codex / "config.toml", codex / "magpie-models.json", oc / "opencode.json"]
before = {p: p.read_bytes() for p in files}
assert main(["-d", "defs/example.toml"]) == 0
assert all(p.read_bytes() == b for p, b in before.items())

# A dry run writes nothing and changes nothing, restore included.
assert main(["-d", "defs/example.toml", "--dry-run"]) == 0
assert all(p.read_bytes() == b for p, b in before.items())
assert main(["-d", "defs/example.toml", "--restore", "--dry-run"]) == 0
assert all(p.read_bytes() == b for p, b in before.items())

# Restore puts the originals back, including values we did not write.
rc = main(["-d", "defs/example.toml", "--restore"])
assert rc == 0, rc
pi_settings = (pi / "settings.json").read_text(encoding="utf-8")
assert '"defaultProvider": "old"' in pi_settings
assert '"defaultModel": "m"' in pi_settings
assert '"enabledModels": ["old/*"]' in pi_settings

droid = (factory / "settings.json").read_text(encoding="utf-8")
assert "custom:mygw/" not in droid
assert "custom:mine" in droid
assert '"model": "custom:mine"' in droid

codex_raw = (codex / "config.toml").read_bytes()
assert b'model = "user-model"' in codex_raw
assert b'model_provider = "openai"' in codex_raw
assert b'name = "old-name"' in codex_raw
assert b'wire_api = "chat"' in codex_raw
assert not (codex / "magpie-models.json").exists()

oc_cfg = (oc / "opencode.json").read_text(encoding="utf-8")
assert '"model": "old/m"' in oc_cfg

# A second restore has nothing stashed and must be a no-op, not a delete.
restored = {p: p.read_bytes() for p in files if p.exists()}
assert main(["-d", "defs/example.toml", "--restore"]) == 0
assert all(p.read_bytes() == s for p, s in restored.items()), "second restore changed files"

# A dry run must not create config directories either.
fresh = Path(tempfile.mkdtemp())
os.environ.update({
    "USERPROFILE": str(fresh), "HOME": str(fresh),
    "XDG_CONFIG_HOME": str(fresh / ".config"),
    "CODEX_HOME": str(fresh / "codex"),
    "FACTORY_HOME_OVERRIDE": str(fresh / "factory"),
})
assert main(["-d", "defs/example.toml", "-a", "pi,droid,codex,opencode", "--dry-run"]) == 0
for created in (fresh / ".pi", fresh / ".config", fresh / "codex", fresh / "factory"):
    assert not created.exists(), f"dry run created {created}"

# A model id typo in [agents] is a hard error, not a silent no-op.
bad = Path(tempfile.mkdtemp()) / "bad.toml"
bad.write_text(
    '[provider]\nid = "mygw"\n\n[gateway]\nbase_url = "http://127.0.0.1:1/v1"\n\n'
    '[[models]]\nid = "m/one"\n\n[agents]\npi = "m/typo"\n', encoding="utf-8")
try:
    load_definition(bad)
except SystemExit as exc:
    assert "m/typo" in str(exc), exc
else:
    raise AssertionError("a model id typo was accepted")

print("integration OK")
