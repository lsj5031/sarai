import tempfile
from pathlib import Path

from injector.textjson import JsonFile

p = Path(tempfile.mkdtemp()) / "opencode.json"
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
print(out)
assert "// keep this comment" in out
assert "// provider comment" in out
assert f.get("model") == "mygw/m2"
assert f.get("provider", "mygw", "options", "headers") == {"a": 1}
print("OK")
