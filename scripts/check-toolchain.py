#!/usr/bin/env python3
"""Check shared tool pins and, by default, the toolchain selected by mise."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--static', action='store_true', help='Check configuration without running tools')
    args = parser.parse_args()
    config = tomllib.loads((ROOT / '.mise/config.toml').read_text())
    tools = config['tools']
    package = json.loads((ROOT / 'package.json').read_text())
    rust = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']
    if tools['node'] != (ROOT / '.nvmrc').read_text().strip():
        raise ValueError('mise Node version must match .nvmrc')
    if 'pnpm@' + tools['npm:pnpm'] != package['packageManager']:
        raise ValueError('mise pnpm version must match packageManager')
    major = int(tools['node'].split('.')[0])
    if package['engines']['node'] != f'>={major} <{major + 1}':
        raise ValueError('Node engine range must include the pinned major')
    if 'rust' not in config['settings']['idiomatic_version_file_enable_tools'] or 'rust' in tools:
        raise ValueError('Read the single Rust pin from rust-toolchain.toml')
    if rust['profile'] != 'minimal' or not {'rustfmt', 'clippy'} <= set(rust['components']):
        raise ValueError('Rust must include the minimal profile, rustfmt and clippy')
    expected = dict(node=tools['node'], pnpm=tools['npm:pnpm'], python=tools['python'], cmake=tools['cmake'], rust=rust['channel'])
    if any(not re.fullmatch(r'\d+\.\d+\.\d+', version) for version in expected.values()):
        raise ValueError('Pin exact stable tool versions')
    if not args.static:
        commands = {'node': ['node', '--version'], 'pnpm': ['pnpm', '--version'],
                    'python': ['python3', '--version'], 'cmake': ['cmake', '--version'],
                    'rust': ['rustc', '--version']}
        for name, command in commands.items():
            text = subprocess.check_output(command, text=True).splitlines()[0]
            match = re.search(r'(?<!\d)(\d+\.\d+\.\d+)(?!\d)', text)
            if not match or match[1] != expected[name]:
                raise ValueError(f'{name} does not use the project pin: {text}')
        components = subprocess.check_output(['rustup', 'component', 'list', '--installed'], text=True).splitlines()
        if any(not any(c.startswith(name + '-') for c in components) for name in ('rustfmt', 'clippy')):
            raise ValueError('mise did not install the required Rust components')
    print(json.dumps(dict(status='passed', runtime_checked=not args.static, tools=expected)))


if __name__ == '__main__':
    main()
