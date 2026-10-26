#!/usr/bin/env python3
"""Exercise post-install native cleanup and Tauri relaunch using a CI-only build.

Does not download/install a release or replace the user's installed app. The
ordinary app is verified separately; this clone adds only an opt-in probe hook.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def wait_for(predicate, message, seconds=100):
    deadline = time.monotonic() + seconds
    while not predicate():
        if time.monotonic() >= deadline:
            raise TimeoutError(message)
        time.sleep(0.1)


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--app', type=Path, required=True)
    p.add_argument('--model', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.uname().sysname != 'Darwin':
        raise SystemExit('Run only on disposable GHA macOS hosts.')
    args.output.mkdir(parents=True, exist_ok=True)
    helper = args.output / 'request-quit'
    subprocess.run(['xcrun', 'swiftc', 'scripts/macos-request-quit.swift', '-o', str(helper)], check=True)
    profile = Path.home() / 'Library/Application Support/dev.toyb0x.voiceinput/settings.json'
    original = profile.read_bytes() if profile.exists() else None
    profile.parent.mkdir(parents=True, exist_ok=True)
    reports = []
    try:
        for backend in ['none', 'cpu', 'metal']:
            root = (args.output / backend).resolve()
            root.mkdir()
            profile.write_text(json.dumps(dict(version=1, shortcut='Command+Shift+Space',
                backend=backend if backend != 'none' else 'metal',
                modelPath=str(args.model.resolve()) if backend != 'none' else '')))
            profile.chmod(0o600)
            report = dict(backend=backend, sourceRevision=os.environ['GITHUB_SHA'],
                          validation='native cleanup and Tauri relaunch; no package install')
            process = None
            started = time.time()
            try:
                with (root / 'launcher.log').open('wb') as log:
                    process = subprocess.Popen([str(args.app.resolve() / 'Contents/MacOS/voice-input'),
                        '--ci-restart-probe', str(root), backend], stdout=log, stderr=subprocess.STDOUT)
                    report['returnCode'] = process.wait(timeout=120)
                if report['returnCode'] != 0:
                    raise RuntimeError('Original app crashed during restart')
                wait_for(lambda: (root / 'relaunched-ready').exists(), 'Relaunched app did not become ready')
                old_pid = json.loads((root / 'initial.json').read_text())['pid']
                new_pid = json.loads((root / 'relaunched.json').read_text())['pid']
                if old_pid != process.pid or new_pid == old_pid or not alive(new_pid):
                    raise RuntimeError('A distinct relaunched app was not observed')
                stages = (root / f'process-{old_pid}.log').read_text().splitlines()
                expected = ['exit-cleanup-started', 'audio-workers-stopped',
                            'exit-cleanup-completed', 'restart-cleanup-ready', 'exit-ready']
                positions = [stages.index(stage) for stage in expected]
                if positions != sorted(positions):
                    raise RuntimeError('Tauri restart preceded native resource cleanup')
                subprocess.run([str(helper), str(new_pid), 'quit'], check=True, timeout=15)
                wait_for(lambda: not alive(new_pid), 'Relaunched app did not quit', seconds=30)
                new_stages = (root / f'process-{new_pid}.log').read_text()
                if 'exit-cleanup-completed' not in new_stages or 'exit-ready' not in new_stages:
                    raise RuntimeError('Relaunched app did not clean up before quit')
                time.sleep(1)  # allow the OS crash reporter to create its report
                crashes = [p for p in (Path.home() / 'Library/Logs/DiagnosticReports').glob('*.ips')
                    if p.name.lower().replace(' ', '').startswith(('voice-input', 'voiceinput'))
                    and p.stat().st_mtime >= started]
                for crash in crashes:
                    shutil.copy2(crash, root / crash.name)
                if crashes or (root / 'error.txt').exists():
                    raise RuntimeError('A crash or probe error was reported')
                report.update(status='passed', initialPid=old_pid, relaunchedPid=new_pid,
                              cleanupBeforeRestart=True, relaunchedModelReady=True)
            except Exception as error:
                report.update(status='failed', error=str(error))
            finally:
                if process is not None and process.poll() is None:
                    process.kill()
                    process.wait()
                if (root / 'relaunched.json').exists():
                    pid = json.loads((root / 'relaunched.json').read_text())['pid']
                    if alive(pid):
                        os.kill(pid, 9)
                        report['forcedCleanup'] = True
                reports.append(report)
                (root / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
            if report['status'] != 'passed':
                raise RuntimeError(f'Failed {backend} restart; inspect restart artifact')
    finally:
        (args.output / 'summary.json').write_text(json.dumps(reports, indent=2) + '\n')
        if original is None:
            profile.unlink(missing_ok=True)
        else:
            profile.write_bytes(original)
            profile.chmod(0o600)


if __name__ == '__main__':
    main()
