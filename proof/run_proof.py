"""End-to-end proof: real codex and droid CLIs, real injected configs.

Start proof/fake_gateway.py first. This script gives each CLI an isolated
HOME/CODEX_HOME/FACTORY_HOME_OVERRIDE, runs the injector, then runs each CLI
non-interactively and checks that its request reached our fake endpoint with
our model id and our API key.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

PROOF = Path(__file__).resolve().parent
ROOT = PROOF.parent
sys.path.insert(0, str(ROOT))

from injector.__main__ import main  # noqa: E402

KEY = "PROOF-KEY-123"
MODEL = "proof/echo-model"


def show(title: str, path: Path) -> None:
    print(f"\n--- {title} ({path}) ---")
    print(path.read_text(encoding="utf-8")[:2500])


def main_proof() -> int:
    log = PROOF / "gateway.log"
    log.write_text("", encoding="utf-8")

    home = Path(tempfile.mkdtemp(prefix="aci-proof-"))
    codex_home = home / "codex"
    factory_home = home / "factory"
    env = os.environ.copy()
    env.update({
        "USERPROFILE": str(home),
        "HOME": str(home),
        "CODEX_HOME": str(codex_home),
        "FACTORY_HOME_OVERRIDE": str(factory_home),
        "NO_COLOR": "1",
    })
    os.environ.update(env)

    rc = main([
        "-d", str(PROOF / "proof_definition.toml"),
        "-a", "codex,droid",
        "--no-stash",
    ])
    if rc:
        return rc

    show("injected Codex config", codex_home / "config.toml")
    show("injected Droid settings", factory_home / ".factory" / "settings.json")

    runs = [
        ("codex", ["codex", "exec", "--skip-git-repo-check",
                   "--dangerously-bypass-approvals-and-sandbox",
                   "Reply with exactly PROOF-OK"]),
        ("droid", ["droid", "exec", "--auto", "low",
                   "Reply with exactly PROOF-OK"]),
    ]
    for name, cmd in runs:
        print(f"\n--- running {name} ---")
        try:
            done = subprocess.run(cmd, cwd=home, env=env, capture_output=True,
                                  text=True, timeout=180)
            print(f"exit: {done.returncode}")
            print((done.stdout or "")[-1200:])
            if done.stderr:
                print("stderr:", done.stderr[-1200:])
        except subprocess.TimeoutExpired as exc:
            print(f"TIMEOUT: {exc}")

    lines = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines() if line]
    print(f"\n--- fake gateway received {len(lines)} request(s) ---")
    for entry in lines:
        print(f"{entry['method']} {entry['path']}  auth={entry['authorization'] or entry['api_key']}")
        print(f"  body: {entry['body'][:220]}")

    posts = [e for e in lines if e["method"] == "POST"]
    by_path = {e["path"].split("?")[0] for e in posts}
    ok = True
    for expected in ("/v1/responses", "/v1/chat/completions"):
        hit = any(p.endswith(expected) for p in by_path)
        print(f"{'PASS' if hit else 'FAIL'}: request to {expected}")
        ok &= hit
    auth_ok = all(
        KEY in ((e["authorization"] or "") + (e["api_key"] or "")) for e in posts
    )
    model_ok = all(e.get("model") == MODEL for e in posts)
    print(f"{'PASS' if auth_ok else 'FAIL'}: our API key reached the endpoint")
    print(f"{'PASS' if model_ok else 'FAIL'}: our model id reached the endpoint")
    return 0 if ok and auth_ok and model_ok else 1


if __name__ == "__main__":
    raise SystemExit(main_proof())
