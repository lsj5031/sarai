import os
import tempfile
from pathlib import Path

from injector.__main__ import main

home = Path(tempfile.mkdtemp())
os.environ["USERPROFILE"] = str(home)
os.environ["HOME"] = str(home)

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

# Codex
codex = home / ".codex"
codex.mkdir()
(codex / "config.toml").write_text(
    '# my config\nmodel = "old"\nmodel_provider = "openai"\n', encoding="utf-8")

# OpenCode
oc = home / ".config" / "opencode"
oc.mkdir(parents=True)
(oc / "opencode.json").write_text(
    '{\n  // my opencode\n  "model": "old/m",\n  "small_model": "old/s"\n}\n', encoding="utf-8")

rc = main(["-d", "defs/example.toml"])
assert rc == 0, rc

assert '"defaultProvider": "mygw"' in (pi / "settings.json").read_text(encoding="utf-8")
assert "// my settings" in (pi / "settings.json").read_text(encoding="utf-8")
assert '"mygw/deepseek/deepseek-v4-pro"' in (pi / "settings.json").read_text(encoding="utf-8")
pi_models = (pi / "models.json").read_text(encoding="utf-8")
assert "// my models" in pi_models and '"other"' in pi_models and '"mygw"' in pi_models

droid = (factory / "settings.json").read_text(encoding="utf-8")
assert "custom:mine" in droid and "custom:mygw/deepseek/deepseek-v4-pro" in droid
assert '"sessionDefaultSettings": {"model": "custom:mygw/deepseek/deepseek-v4-pro"}' in droid

codex_cfg = (codex / "config.toml").read_text(encoding="utf-8")
assert "# my config" in codex_cfg
assert 'model_provider = "mygw"' in codex_cfg
assert "[model_providers.mygw]" in codex_cfg
assert (codex / "magpie-models.json").exists()

oc_cfg = (oc / "opencode.json").read_text(encoding="utf-8")
assert "// my opencode" in oc_cfg
assert '"model": "mygw/deepseek/deepseek-v4-pro"' in oc_cfg
assert '"mygw"' in oc_cfg

# Idempotence: a second apply must not change bytes.
before = {p: p.read_bytes() for p in [
    pi / "settings.json", pi / "models.json", factory / "settings.json",
    codex / "config.toml", oc / "opencode.json"]}
main(["-d", "defs/example.toml"])
assert all(p.read_bytes() == b for p, b in before.items())

# Restore puts the originals back.
rc = main(["-d", "defs/example.toml", "--restore"])
assert rc == 0, rc
assert '"defaultProvider": "old"' in (pi / "settings.json").read_text(encoding="utf-8")
assert 'custom:mygw/' not in (factory / "settings.json").read_text(encoding="utf-8")
assert 'model_provider = "openai"' in (codex / "config.toml").read_text(encoding="utf-8")
assert not (codex / "magpie-models.json").exists()
assert '"model": "old/m"' in (oc / "opencode.json").read_text(encoding="utf-8")
print("integration OK")
