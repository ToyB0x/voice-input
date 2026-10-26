#!/usr/bin/env python3
"""Install only the checksum-pinned Japanese font needed by Linux browser evidence."""
import argparse
import hashlib
from pathlib import Path
import subprocess
import urllib.request

REVISION = '66a36c8c94b1a5d992ee4e7f392fccfe4945767c'  # google/fonts, 2026-03-25 (>48h)
SHA256 = 'c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f'
URL = f'https://raw.githubusercontent.com/google/fonts/{REVISION}/ofl/notosansjp/NotoSansJP%5Bwght%5D.ttf'

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--directory',type=Path,default=Path.home()/'.local/share/fonts/voice-input')
    a=p.parse_args();a.directory.mkdir(parents=True,exist_ok=True)
    target=a.directory/'NotoSansJP.ttf'
    data=target.read_bytes() if target.exists() else b''
    if hashlib.sha256(data).hexdigest()!=SHA256:
        with urllib.request.urlopen(URL,timeout=45) as response: data=response.read(12_000_001)
        if len(data)>12_000_000 or hashlib.sha256(data).hexdigest()!=SHA256:
            raise RuntimeError('Presentation font checksum or size mismatch')
        target.write_bytes(data)
    subprocess.run(['fc-cache','-f',str(a.directory)],check=True,timeout=30)
    print('Noto Sans JP installed with verified SHA-256; SIL Open Font License 1.1')
if __name__=='__main__': main()
