#!/usr/bin/env python3
"""Build a macOS 14-compatible, weak-linked Foundation Models helper. No model download."""
import argparse
import os
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--require-framework', action='store_true')
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        raise SystemExit('The Foundation Models helper is built on macOS.')
    sdk = Path(subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-path'], text=True).strip())
    framework = (sdk / 'System/Library/Frameworks/FoundationModels.framework').exists()
    if args.require_framework and not framework:
        raise SystemExit('FoundationModels requires the macOS 26 SDK / Xcode 26 or later.')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    command = ['xcrun', 'swiftc', '-parse-as-library', '-O', '-target',
               f'{platform.machine()}-apple-macos14.0', str(ROOT / 'scripts/apple-foundation-correction.swift'),
               '-o', str(args.output)]
    if framework:
        command += ['-Xlinker', '-weak_framework', '-Xlinker', 'FoundationModels']
    subprocess.run(command, check=True, env=dict(os.environ, MACOSX_DEPLOYMENT_TARGET='14.0'))
    subprocess.run(['codesign', '--force', '--sign', '-', str(args.output)], check=True)
    print('Foundation Models helper built; framework compiled: ' + str(framework))


if __name__ == '__main__':
    main()
