#!/usr/bin/env python3
"""One-time owner setup. Run locally; never paste a private key into a chat or commit."""
import argparse
import json
import os
from pathlib import Path
import subprocess

REPO='ToyB0x/voice-input'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--key-file',type=Path,required=True,help='Permanent backup path outside the repository')
    args=parser.parse_args()
    root=Path(__file__).resolve().parent.parent
    key=args.key_file.expanduser().resolve()
    if key.is_relative_to(root) or key.exists() or Path(str(key)+'.pub').exists():
        raise RuntimeError('Choose a new key backup path outside the repository; existing files are never overwritten')
    # Check administration access and refuse implicit key rotation before generating anything.
    for kind, name in [('variables','TAURI_UPDATER_PUBLIC_KEY'),('secrets','TAURI_SIGNING_PRIVATE_KEY')]:
        result=subprocess.run(['gh','api',f'repos/{REPO}/actions/{kind}'],capture_output=True,text=True)
        if result.returncode:
            raise RuntimeError('GitHub Secrets/Variables administration access is required. Run as the repository owner')
        if any(item['name']==name for item in json.loads(result.stdout)[kind]):
            raise RuntimeError('Updater signing is already configured. Refusing to rotate or overwrite existing keys')
    key.parent.mkdir(parents=True,exist_ok=True)
    previous=os.umask(0o077)
    try:
        result=subprocess.run(['npm','exec','tauri','signer','generate','--','--ci','--password','','--write-keys',str(key)],cwd=root,capture_output=True)
        if result.returncode:
            raise RuntimeError('Tauri key generation failed; its output is intentionally hidden')
        key.chmod(0o600)
        public=Path(str(key)+'.pub').read_text().strip()
        subprocess.run(['gh','secret','set','TAURI_SIGNING_PRIVATE_KEY','--repo',REPO],input=key.read_bytes(),check=True)
        subprocess.run(['gh','secret','set','TAURI_SIGNING_PRIVATE_KEY_PASSWORD','--repo',REPO],input=b'',check=True)
        subprocess.run(['gh','variable','set','TAURI_UPDATER_PUBLIC_KEY','--repo',REPO,'--body',public],check=True)
    finally:
        os.umask(previous)
    print('Signing configured. Keep the private key backup secure; retain it for all future updates.')

if __name__=='__main__':
    try: main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error))
