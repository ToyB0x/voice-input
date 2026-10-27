#!/usr/bin/env python3
"""Explicit pinned-model setup for the public image evaluation (not app startup)."""
import argparse
import datetime
import json
import os
import subprocess
import urllib.request
from pathlib import Path
from transcription_models import checksum
ROOT = Path(__file__).resolve().parents[1]

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--directory',type=Path,required=True)
    p.add_argument('--source',type=Path,required=True)
    args=p.parse_args()
    spec=json.loads((ROOT/'evaluation/local-vlm/model.json').read_text())
    if args.source.exists():
        if subprocess.check_output(['git','-C',str(args.source),'rev-parse','HEAD'],text=True).strip()!=spec['runtime']['revision']:
            raise ValueError('Unexpected runtime source; use an empty setup directory')
        if subprocess.check_output(['git','-C',str(args.source),'status','--porcelain','--untracked-files=no'],text=True).strip():
            raise ValueError('Modified runtime source')
    else:
        args.source.mkdir(parents=True)
        for command in [['init','-q'],['remote','add','origin',spec['runtime']['repo']],
                        ['fetch','--depth','1','origin',spec['runtime']['revision']],['checkout','--detach','FETCH_HEAD']]:
            subprocess.run(['git','-C',str(args.source),*command],check=True)
    committed=subprocess.check_output(['git','-C',str(args.source),'show','-s','--format=%cI','HEAD'],text=True).strip()
    date=datetime.datetime.fromisoformat(committed)
    if datetime.datetime.now(datetime.timezone.utc)-date < datetime.timedelta(hours=48):
        raise ValueError('Runtime must satisfy 48-hour cooling')
    args.directory.mkdir(parents=True,exist_ok=True)
    for entry in spec['files']:
        target=args.directory/entry['file']
        if target.exists() and target.stat().st_size==entry['bytes'] and checksum(target)==entry['sha256']:
            continue
        temporary=target.with_suffix('.download')
        try:
            url=f"https://huggingface.co/{spec['repo']}/resolve/{spec['revision']}/{entry['file']}"
            with urllib.request.urlopen(url,timeout=120) as response,temporary.open('wb') as out:
                while chunk:=response.read(1024*1024):out.write(chunk)
            if temporary.stat().st_size!=entry['bytes'] or checksum(temporary)!=entry['sha256']:
                raise ValueError('Model identity mismatch')
            os.replace(temporary,target)
        finally:temporary.unlink(missing_ok=True)
    print('Verified pinned VLM and runtime source')
if __name__=='__main__':main()
