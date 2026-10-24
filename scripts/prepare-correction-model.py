#!/usr/bin/env python3
"""Explicitly download an official, pinned Apache-2.0 correction model. No user data."""
import argparse,os
from pathlib import Path
import urllib.request
from correction_models import MODELS
from transcription_models import checksum
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('destination',type=Path)
parser.add_argument('--model',choices=MODELS,default='qwen3-1.7b')
args=parser.parse_args()
model=MODELS[args.model]
target=args.destination
target.parent.mkdir(parents=True,exist_ok=True)
if target.is_file() and checksum(target)==model['sha256']:
    print('Verified local correction model');raise SystemExit(0)
temporary=target.with_name(target.name+'.download')
try:
    url=f"https://huggingface.co/{model['repo']}/resolve/{model['revision']}/{model['file']}"
    with urllib.request.urlopen(url,timeout=120) as response,temporary.open('wb') as output:
        while chunk:=response.read(1024*1024):output.write(chunk)
    if temporary.stat().st_size!=model['bytes'] or checksum(temporary)!=model['sha256']:
        raise RuntimeError('Correction model checksum mismatch; destination preserved')
    os.replace(temporary,target)
finally:
    temporary.unlink(missing_ok=True)
print('Prepared pinned local correction model')
