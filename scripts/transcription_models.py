"""Pinned multilingual Whisper model identities (MIT), shared by setup/evaluation."""
import hashlib

import json
from pathlib import Path

_CATALOG = json.loads((Path(__file__).resolve().parents[1] / 'crates/local-models/catalog.json').read_text())
REVISION = _CATALOG['transcription']['revision']
MODELS = _CATALOG['transcription']['models']


def checksum(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

