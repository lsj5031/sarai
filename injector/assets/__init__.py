"""Bundled assets.

CODEX_PROMPT is the instruction text Codex sends for a model its catalog
does not know. Copied from yetone/magpie (MIT), which attributes it to
openai/codex (Apache-2.0). A Codex catalog entry must carry a non-empty
base_instructions or the whole catalog is rejected.
"""

from pathlib import Path

CODEX_PROMPT = (Path(__file__).with_name("codex_prompt.md")).read_text(encoding="utf-8")
