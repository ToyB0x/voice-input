#!/usr/bin/env python3
"""Verify real app exit codes on disposable GHA macOS hosts, independent of OCR."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.uname().sysname != 'Darwin':
        raise SystemExit('This probe changes the disposable CI app profile; run only on GHA macOS.')
    args.output.mkdir(parents=True, exist_ok=True)
    executable = args.app.resolve() / 'Contents/MacOS/voice-input'
    helper = args.output / 'request-quit'
    subprocess.run(['xcrun', 'swiftc', 'scripts/macos-request-quit.swift', '-o', str(helper)], check=True)
    profile = Path.home() / 'Library/Application Support/dev.toyb0x.voiceinput/settings.json'
    original = profile.read_bytes() if profile.exists() else None
    profile.parent.mkdir(parents=True, exist_ok=True)
    reports = []
    try:
        for backend in ['none', 'cpu', 'metal']:
            root = args.output / backend
            root.mkdir()
            profile.write_text(json.dumps(dict(version=1, backend=backend if backend != 'none' else 'metal',
                                              modelPath=str(args.model.resolve()) if backend != 'none' else '')))
            profile.chmod(0o600)
            started = time.time()
            log = root / 'app.log'
            report = dict(backend=backend, requestedQuit=False, sourceRevision=os.environ['GITHUB_SHA'])
            process = None
            try:
                with log.open('wb') as output:
                    process = subprocess.Popen([str(executable)], stdout=output, stderr=subprocess.STDOUT,
                                               env=dict(os.environ, VOICE_INPUT_LIFECYCLE_DIAGNOSTICS='1'))
                    subprocess.run([str(helper), str(process.pid), 'ready'], check=True, timeout=15)
                    if backend != 'none':
                        deadline = time.monotonic() + 90
                        while 'voice-input:lifecycle:model-ready' not in log.read_text(errors='replace'):
                            if process.poll() is not None:
                                raise RuntimeError('App exited before model readiness')
                            if time.monotonic() >= deadline:
                                raise TimeoutError('Model readiness was not observed')
                            time.sleep(0.1)
                    if process.poll() is not None:
                        raise RuntimeError('App exited without an explicit quit request')
                    subprocess.run([str(helper), str(process.pid), 'quit'], check=True, timeout=15)
                    report['requestedQuit'] = True
                    report['returnCode'] = process.wait(timeout=30)
                    report['status'] = 'passed' if report['returnCode'] == 0 else 'failed'
                    if report['returnCode'] != 0:
                        raise RuntimeError('App crashed or returned nonzero during quit')
            except Exception as error:
                report['status'] = 'failed'
                report['error'] = str(error)
            finally:
                if process is not None:
                    if process.poll() is None:
                        # Cleanup after failure; never count a forced termination as success.
                        report['forcedCleanup'] = True
                        process.kill()
                        process.wait()
                    report['returnCode'] = process.returncode
                reports.append(report)
                (root / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
                if report['status'] != 'passed':
                    crash_root = Path.home() / 'Library/Logs/DiagnosticReports'
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        found = False
                        for path in crash_root.glob('*.ips'):
                            if path.name.lower().replace(' ', '').startswith(('voice-input', 'voiceinput')) and path.stat().st_mtime >= started:
                                shutil.copy2(path, root / path.name)
                                found = True
                        if found:
                            break
                        time.sleep(0.2)
            if report['status'] != 'passed':
                raise RuntimeError(f'Unclean {backend} app exit; inspect exit artifact')
    finally:
        (args.output / 'summary.json').write_text(json.dumps(reports, indent=2) + '\n')
        if original is None:
            profile.unlink(missing_ok=True)
        else:
            profile.write_bytes(original)
            profile.chmod(0o600)


if __name__ == '__main__':
    main()
