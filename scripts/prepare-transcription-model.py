#!/usr/bin/env python3
"""Explicit setup only. Download pinned, checksum-verified Whisper models (MIT).

Inference never calls this script. No audio or transcripts are read here.
"""
import argparse
import os
from pathlib import Path
import urllib.request

from transcription_models import MODELS, REVISION, checksum


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--model", choices=MODELS, default="base")
    args = parser.parse_args()
    model = MODELS[args.model]
    url = f"https://huggingface.co/ggerganov/whisper.cpp/resolve/{REVISION}/{model['file']}"
    target = args.destination
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_file() and checksum(target) == model["sha256"]:
        print(f"Verified: {target}")
        return
    temporary = target.with_name(target.name + ".download")
    try:
        with urllib.request.urlopen(url, timeout=120) as response, temporary.open("wb") as out:
            while chunk := response.read(1024 * 1024):
                out.write(chunk)
        if temporary.stat().st_size != model["bytes"] or checksum(temporary) != model["sha256"]:
            raise RuntimeError("Model checksum mismatch; destination was not replaced")
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    print(f"Prepared: {target}")


if __name__ == "__main__":
    main()
