#!/usr/bin/env python3
"""Reject registry dependencies published less than 48 hours ago, before building them.

Cargo has no stable minimum-release-age resolver option. Check every registry package in
all app/helper lockfiles, including target-specific and transitive dependencies. pnpm's
native policy checks its own dependencies; this additionally checks its pinned bootstrap.
Only public package metadata is fetched. No package code or build script is executed.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import time
import tomllib
import urllib.error
import urllib.request
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
MINIMUM_AGE = dt.timedelta(days=2)
CRATES_IO = 'registry+https://github.com/rust-lang/crates.io-index'


def timestamp(value):
    if not isinstance(value, str):
        raise ValueError('Missing release timestamp')
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    if parsed.tzinfo is None:
        raise ValueError('Release timestamp must have a timezone')
    return parsed


def cooled(published, now):
    return timestamp(published) <= now - MINIMUM_AGE


def index_path(name):
    if not re.fullmatch(r'[a-zA-Z0-9_-]+', name):
        raise ValueError('Invalid registry package name')
    name = name.lower()
    if len(name) < 3:
        return f'{len(name)}/{name}'
    if len(name) == 3:
        return f'3/{name[0]}/{name}'
    return f'{name[:2]}/{name[2:4]}/{name}'


def packages(root):
    locks = [root / 'src-tauri/Cargo.lock', *sorted(root.glob('crates/*/Cargo.lock'))]
    if (root / 'Cargo.lock').exists():
        locks.append(root / 'Cargo.lock')
    result = set()
    for lock in locks:
        for package in tomllib.loads(lock.read_text())['package']:
            source = package.get('source')
            if source is None:  # Repository-owned path dependencies have no publication date.
                continue
            if source != CRATES_IO:
                raise ValueError(f'Unsupported dependency source in {lock.relative_to(root)}: {source}')
            result.add((package['name'], package['version'], package['checksum']))
    return sorted(result), locks


class Metadata:
    def __init__(self, cache):
        self.cache = cache
        cache.mkdir(parents=True, exist_ok=True)

    def get(self, url, refresh=False):
        path = self.cache / (hashlib.sha256(url.encode()).hexdigest() + '.json')
        # A cached immutable version timestamp can only get older. If a new version is
        # absent from an older index, callers refresh it before treating it as missing.
        if path.exists() and not refresh:
            return path.read_bytes()
        request = urllib.request.Request(url, headers={'User-Agent': 'voice-input-release-age-check/1'})
        for attempt in range(4):
            try:
                with urllib.request.urlopen(request, timeout=30) as response:
                    content = response.read(64 * 1024 * 1024 + 1)
                if len(content) > 64 * 1024 * 1024:
                    raise ValueError('Registry metadata exceeds size limit')
                temporary = path.with_suffix(f'.{os.getpid()}.tmp')
                temporary.write_bytes(content)
                temporary.replace(path)
                return content
            except (urllib.error.URLError, TimeoutError):
                if attempt == 3:
                    raise
                time.sleep(min(2 ** attempt, 4))
        raise AssertionError('unreachable')

    def crate(self, name, versions):
        url = 'https://index.crates.io/' + index_path(name)
        entries = {v['vers']: v for v in map(json.loads, self.get(url).splitlines())}
        if any(version not in entries for version in versions):
            entries = {v['vers']: v for v in map(json.loads, self.get(url, refresh=True).splitlines())}
        return entries

    def npm_release(self, name, version):
        url = 'https://registry.npmjs.org/' + urllib.parse.quote(name, safe='@')
        metadata = json.loads(self.get(url))
        if version not in metadata.get('time', {}):
            metadata = json.loads(self.get(url, refresh=True))
        return metadata.get('time', {}).get(version), metadata.get('versions', {}).get(version, {})


def check_package(name, version, checksum, entry, now):
    if entry is None:
        raise ValueError(f'{name}@{version}: missing registry version')
    if entry.get('cksum') != checksum:
        raise ValueError(f'{name}@{version}: registry/lockfile checksum mismatch')
    published = entry.get('pubtime')
    if not cooled(published, now):
        ready = timestamp(published) + MINIMUM_AGE
        raise ValueError(f'{name}@{version}: cooling until {ready.isoformat()}')
    return {'name': name, 'version': version, 'published': published, 'checksum': checksum}


def check(root, metadata, now):
    locked, locks = packages(root)
    grouped = {}
    for name, version, checksum in locked:
        grouped.setdefault(name, []).append((version, checksum))

    def inspect(item):
        name, versions = item
        records, errors = [], []
        try:
            entries = metadata.crate(name, [v for v, _ in versions])
            for version, checksum in versions:
                try:
                    records.append(check_package(name, version, checksum, entries.get(version), now))
                except (ValueError, KeyError, TypeError) as error:
                    errors.append(f'{name}@{version}: {error}')
        except Exception as error:
            errors.append(f'{name}: unable to verify publication metadata ({type(error).__name__})')
        return records, errors

    records, errors = [], []
    with ThreadPoolExecutor(max_workers=8) as pool:
        for entries, failures in pool.map(inspect, sorted(grouped.items())):
            records.extend(entries)
            errors.extend(failures)
    package_manager = json.loads((root / 'package.json').read_text())['packageManager']
    if not re.fullmatch(r'pnpm@\d+\.\d+\.\d+', package_manager):
        raise ValueError('Pin an exact stable pnpm version in packageManager')
    version = package_manager.split('@')[1]
    try:
        published, release = metadata.npm_release('pnpm', version)
        if not cooled(published, now):
            errors.append(f'{package_manager}: release has not cooled for 48 hours')
        # pnpm 12 is a native wrapper: its platform companions are executed during
        # bootstrap, before the project install can enforce its own pnpm policy.
        companions = release.get('optionalDependencies', {})
        def inspect_tool(item):
            name, version = item
            if not re.fullmatch(r'\d+\.\d+\.\d+', version):
                return f'{name}: pnpm bootstrap dependency must pin a stable version'
            published, _ = metadata.npm_release(name, version)
            return None if cooled(published, now) else f'{name}@{version}: pnpm companion has not cooled for 48 hours'
        with ThreadPoolExecutor(max_workers=8) as pool:
            errors.extend(error for error in pool.map(inspect_tool, sorted(companions.items())) if error)
    except Exception as error:
        errors.append(f'{package_manager}: unable to verify publication metadata ({type(error).__name__})')
    return dict(checkedAt=now.isoformat(), minimumAgeHours=48,
                lockfiles=[str(p.relative_to(root)) for p in locks],
                packages=records, packageManager=package_manager, errors=errors)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache-dir', type=Path, default=Path.home() / '.cache/voice-input-release-metadata')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    report = check(ROOT, Metadata(args.cache_dir), dt.datetime.now(dt.timezone.utc))
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + '\n')
    if report['errors']:
        raise SystemExit('\n'.join(report['errors']))
    print(f"PASS: {len(report['packages'])} Rust registry versions across {len(report['lockfiles'])} lockfiles and {report['packageManager']} are at least 48 hours old")


if __name__ == '__main__':
    main()
