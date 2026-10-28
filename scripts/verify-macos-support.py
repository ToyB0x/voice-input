#!/usr/bin/env python3
"""Verify the actual build host, SDK and packaged executables meet the supported OS."""
import argparse
import json
from pathlib import Path
import plistlib
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def version(value):
    return tuple((list(map(int, value.split('.'))) + [0, 0])[:3])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    minimum = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())['bundle']['macOS']['minimumSystemVersion']
    host = subprocess.check_output(['sw_vers', '-productVersion'], text=True).strip()
    sdk = subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-version'], text=True).strip()
    report = dict(minimumSystemVersion=minimum, host=host, sdk=sdk, binaries={})
    try:
        if version(host) < version(minimum) or version(sdk) < version(minimum):
            raise ValueError(f'Build/launch verification requires macOS and SDK {minimum} or later')
        if args.app:
            with (args.app / 'Contents/Info.plist').open('rb') as source:
                info = plistlib.load(source)
            if version(info.get('LSMinimumSystemVersion', '0')) != version(minimum):
                raise ValueError('Bundle minimum OS differs from tauri.conf.json')
            paths = [Path('MacOS') / info['CFBundleExecutable']]
            paths += [Path('Resources/bin') / name for name in
                      ['local-correction', 'apple-foundation-correction', 'compare-transcription', 'context-lab-native', 'llama-mtmd-cli', 'llama-completion', 'llama-server']]
            for relative in paths:
                output = subprocess.check_output(['xcrun', 'vtool', '-show-build',
                                                  str(args.app / 'Contents' / relative)], text=True)
                targets = re.findall(r'^\s*minos\s+([\d.]+)', output, re.MULTILINE)
                report['binaries'][str(relative)] = targets
                if not targets or any(version(target) != version(minimum) for target in targets):
                    raise ValueError(f'Executable minimum OS differs from configured minimum: {relative}')
        report['status'] = 'passed'
    except Exception:
        report['status'] = 'failed'
        raise
    finally:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report))


if __name__ == '__main__':
    main()
