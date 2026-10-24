"""Official Apache-2.0 Qwen3 GGUF identities; explicit setup only."""
import json
from pathlib import Path

MODELS = json.loads((Path(__file__).resolve().parents[1] / 'crates/local-models/catalog.json').read_text())['correction']['models']
