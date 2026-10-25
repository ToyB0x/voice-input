#!/usr/bin/env python3
"""Prepare native Tauri release assets. No upload or signing-key access here."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import shutil

REPO = 'ToyB0x/voice-input'

def version(tag):
    if not re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag):
        raise ValueError('Use a stable SemVer tag such as v0.0.1')
    return tag[1:]

def set_version(root, tag):
    value = version(tag)
    for name in ['package.json', 'src-tauri/tauri.conf.json']:
        path = root / name
        data = json.loads(path.read_text())
        data['version'] = value
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2)+'\n')
    path = root / 'src-tauri/Cargo.toml'
    path.write_text(re.sub(r'(?m)^version = "[^"]+"', f'version = "{value}"', path.read_text(), count=1))
    path = root / 'src-tauri/Cargo.lock'
    text = path.read_text()
    text, count = re.subn(r'(\[\[package\]\]\nname = "voice-input"\nversion = ")[^"]+', lambda m: m[1]+value, text)
    if count != 1:
        raise ValueError('App package is missing from Cargo.lock')
    path.write_text(text)

def package(bundle, destination, tag):
    value = version(tag)
    destination.mkdir(parents=True, exist_ok=False)
    archives = list(bundle.rglob('*.app.tar.gz'))
    dmgs = list(bundle.rglob('*.dmg'))
    if len(archives) != 1 or len(dmgs) != 1:
        raise ValueError('Expected exactly one Tauri updater archive and one DMG')
    archive = archives[0]
    signature = Path(str(archive)+'.sig').read_text().strip()
    if not signature or len(signature) > 4096:
        raise ValueError('Missing update signature')
    name = f'Voice-Input_{value}_aarch64'
    update_name = name+'.app.tar.gz'
    shutil.copy2(archive, destination/update_name)
    (destination/(update_name+'.sig')).write_text(signature+'\n')
    shutil.copy2(dmgs[0], destination/(name+'.dmg'))
    manifest = dict(version=value, notes=f'Voice Input {value}', pub_date=dt.datetime.now(dt.timezone.utc).isoformat().replace('+00:00','Z'), platforms={'darwin-aarch64':dict(url=f'https://github.com/{REPO}/releases/download/{tag}/{update_name}', signature=signature)})
    (destination/'latest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    checksums = {p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in destination.iterdir()}
    (destination/'SHA256SUMS').write_text(''.join(f'{digest}  {name}\n' for name,digest in sorted(checksums.items())))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    setup = sub.add_parser('version');setup.add_argument('--tag', required=True)
    build = sub.add_parser('package');build.add_argument('--tag', required=True);build.add_argument('--bundle', type=Path, required=True);build.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'version':
        set_version(Path(__file__).resolve().parent.parent, args.tag)
    else:
        package(args.bundle, args.output, args.tag)
