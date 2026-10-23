#!/usr/bin/env python3
"""Explicit setup only. Download pinned, checksum-verified Whisper base (MIT).

Inference never calls this script. No audio or transcripts are read here.
"""
import hashlib
import os
from pathlib import Path
import sys
import urllib.request

REVISION = "5359861c739e955e79d9a303bcbc70fb988958b1"
SHA256 = "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
URL = f"https://huggingface.co/ggerganov/whisper.cpp/resolve/{REVISION}/ggml-base.bin"


def checksum(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: prepare-transcription-model.py DESTINATION.bin")
    target = Path(sys.argv[1])
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_file() and checksum(target) == SHA256:
        print(f"Verified: {target}")
        return
    temporary = target.with_name(target.name + ".download")
    try:
        with urllib.request.urlopen(URL, timeout=120) as response, temporary.open("wb") as out:
            while chunk := response.read(1024 * 1024):
                out.write(chunk)
        if checksum(temporary) != SHA256:
            raise RuntimeError("Model checksum mismatch; destination was not replaced")
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    print(f"Prepared: {target}")


if __name__ == "__main__":
    main()
