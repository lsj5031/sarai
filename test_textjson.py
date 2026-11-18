import json
import tempfile
from pathlib import Path

from injector.textjson import JsonFile
from injector.tomlfile import TomlFile

base = Path(tempfile.mkdtemp())

# --- JSONC: splice the edited value, leave surrounding comments alone ------
p = base / "opencode.json"
p.write_text(
    '{\n'
    '  // keep this comment\n'
    '  "model": "old/m",\n'
    '  "provider": {\n'
    '    // provider comment\n'
    '    "mygw": {"options": {"baseURL": "x"}}\n'
    '  }\n'
    '}\n',
    encoding="utf-8",
)
f = JsonFile(p)
assert f.get("model") == "old/m"
assert f.get("provider", "mygw", "options", "baseURL") == "x"
f.set("mygw/m2", "model")
f.set("http://new/v1", "provider", "mygw", "options", "baseURL")
f.set({"a": 1}, "provider", "mygw", "options", "headers")
assert f.delete("provider", "mygw", "options", "baseURL")
f.save()
out = p.read_text(encoding="utf-8")
assert "// keep this comment" in out
assert "// provider comment" in out
assert f.get("model") == "mygw/m2"
assert f.get("provider", "mygw", "options", "headers") == {"a": 1}

# --- Trailing commas must not swallow the separator -----------------------
trail = base / "trail.json"
trail.write_text('{\n  "a": 1,\n}\n', encoding="utf-8")
f = JsonFile(trail)
f.set(2, "b")
assert json.loads(f.src) == {"a": 1, "b": 2}, f.src
f.set({"nested": True}, "c")
assert json.loads(f.src) == {"a": 1, "b": 2, "c": {"nested": True}}, f.src
f.save()
assert json.loads(trail.read_text(encoding="utf-8")) == {
    "a": 1, "b": 2, "c": {"nested": True}}

# --- Empty and comment-only files are empty objects, not crashes ----------
empty = base / "empty.json"
empty.write_text("", encoding="utf-8")
f = JsonFile(empty)
assert f.get("anything") is None
f.set("v", "k")
f.save()
assert json.loads(empty.read_text(encoding="utf-8")) == {"k": "v"}

only_comments = base / "only-comments.json"
only_comments.write_text("// nothing yet\n", encoding="utf-8")
f = JsonFile(only_comments)
f.set(1, "n")
f.save()
assert json.loads(only_comments.read_text(encoding="utf-8")) == {"n": 1}

# --- Values that contain comments still read back as data -----------------
mixed = base / "mixed.json"
mixed.write_text(
    '{\n'
    '  "provider": {\n'
    '    // note\n'
    '    "mygw": {\n'
    '      // inner\n'
    '      "a": 1\n'
    '    }\n'
    '  },\n'
    '  "list": [\n'
    '    // first\n'
    '    {"id": "x"}\n'
    '  ]\n'
    '}\n',
    encoding="utf-8",
)
f = JsonFile(mixed)
assert f.get("provider", "mygw") == {"a": 1}
assert f.get("list") == [{"id": "x"}]
f.set("y", "provider", "mygw", "b")
assert f.get("provider", "mygw") == {"a": 1, "b": "y"}
assert "// inner" in mixed.read_text(encoding="utf-8")

# --- Inserted lines follow the file's line endings ------------------------
crlf = base / "crlf.json"
crlf.write_bytes(b'{\r\n  "a": 1\r\n}\r\n')
f = JsonFile(crlf)
f.set(2, "b")
assert b'\r\n  "b": 2' in f.src.encode("utf-8")
assert "\n" not in f.src.replace("\r\n", "")
f.save()
assert b"\r\n" in crlf.read_bytes()

# --- TOML keeps CRLF and stays idempotent ---------------------------------
t = base / "config.toml"
t.write_bytes(b'model = "old"\r\nmodel_provider = "openai"\r\n')
tf = TomlFile(t)
tf.set("new", "model")
assert tf.save()
raw = t.read_bytes()
assert b'model = "new"\r\n' in raw and b'model_provider = "openai"\r\n' in raw, raw
assert not tf.save()

print("editors OK")
