#!/usr/bin/env python3
"""Build the local-only correction executable; never downloads model weights."""
from pathlib import Path
import json
import os
import platform
import shutil
import subprocess
ROOT=Path(__file__).resolve().parent.parent
if platform.system() != 'Darwin':
    raise SystemExit('The app runtime is built on macOS. The standalone correction crate also supports CPU tests on Linux.')
subprocess.run(['python3',str(ROOT/'scripts/check-dependency-age.py')],check=True)
build_env=dict(os.environ)
minimum=json.loads((ROOT/'src-tauri/tauri.conf.json').read_text())['bundle']['macOS']['minimumSystemVersion']
build_env['MACOSX_DEPLOYMENT_TARGET']=minimum
subprocess.run(['cargo','build','--manifest-path',str(ROOT/'crates/local-correction/Cargo.toml'),'--release','--locked'],check=True,env=build_env)
source=Path(os.environ.get('CARGO_TARGET_DIR',ROOT/'crates/local-correction/target'))/'release/local-correction'
# Refuse to package a diagnostic variant, including after switching Cargo features.
info=json.loads(subprocess.check_output([str(source),'--build-info'],text=True))
if info.get('ci_diagnostics') is not False:
    raise RuntimeError('Refusing to bundle a diagnostic correction runtime')
target=ROOT/'src-tauri/resources/local-correction'
target.parent.mkdir(parents=True,exist_ok=True)
shutil.copy2(source,target)
subprocess.run(['codesign','--force','--sign','-',str(target)],check=True)
print('Local correction runtime prepared (no model weights downloaded).')

foundation=ROOT/'src-tauri/resources/apple-foundation-correction'
prebuilt=os.environ.get('VOICE_INPUT_FOUNDATION_RUNTIME')
if prebuilt:
    shutil.copy2(Path(prebuilt),foundation)
    foundation.chmod(0o755)
else:
    subprocess.run(['python3',str(ROOT/'scripts/build-foundation-runtime.py'),'--output',str(foundation)],check=True)
subprocess.run(['codesign','--force','--sign','-',str(foundation)],check=True)
status=json.loads(subprocess.check_output([str(foundation),'--status'],text=True,timeout=15))
if status.get('protocol_version')!=1:
    raise RuntimeError('Invalid Foundation Models helper protocol')
print('Foundation Models helper prepared: '+status['availability'])

# Share Whisper build artifacts with the app instead of compiling the same native engine twice.
comparison_env=dict(build_env)
comparison_env.setdefault('CARGO_TARGET_DIR',str(ROOT/'src-tauri/target'))
subprocess.run(['cargo','build','--manifest-path',str(ROOT/'crates/local-transcription/Cargo.toml'),'--release','--locked','--bin','compare-transcription'],check=True,env=comparison_env)
comparison_source=Path(comparison_env['CARGO_TARGET_DIR'])/'release/compare-transcription'
comparison_target=ROOT/'src-tauri/resources/compare-transcription'
shutil.copy2(comparison_source,comparison_target)
subprocess.run(['codesign','--force','--sign','-',str(comparison_target)],check=True)
print('In-memory comparison runtime prepared.')

lab=ROOT/'src-tauri/resources/context-lab-native'
subprocess.run(['xcrun','swiftc','-O','-target',f'{platform.machine()}-apple-macos{minimum}',str(ROOT/'scripts/context-lab-native.swift'),'-o',str(lab)],check=True,env=build_env)
subprocess.run(['codesign','--force','--sign','-',str(lab)],check=True)
print('Controlled context lab helper prepared (no user screen capture).')
