"""Pinned multilingual Whisper model identities (MIT), shared by setup/evaluation."""
import hashlib

REVISION = "5359861c739e955e79d9a303bcbc70fb988958b1"
MODELS = {
    "base": {"file": "ggml-base.bin", "bytes": 147951465, "sha256": "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"},
    "small": {"file": "ggml-small.bin", "bytes": 487601967, "sha256": "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"},
    "medium": {"file": "ggml-medium.bin", "bytes": 1533763059, "sha256": "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"},
    "large-v3-turbo": {"file": "ggml-large-v3-turbo.bin", "bytes": 1624555275, "sha256": "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69", "engine_name": "large"},
    "large-v3": {"file": "ggml-large-v3.bin", "bytes": 3095033483, "sha256": "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2", "engine_name": "large"},
}


def checksum(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

