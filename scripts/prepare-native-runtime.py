#!/usr/bin/env python3
"""Build the local-only correction executable; never downloads model weights."""
from pathlib import Path
import os
import platform
import shutil
import subprocess
ROOT=Path(__file__).resolve().parent.parent
if platform.system() != 'Darwin':
    raise SystemExit('The app runtime is built on macOS. The standalone correction crate also supports CPU tests on Linux.')
subprocess.run(['cargo','build','--manifest-path',str(ROOT/'crates/local-correction/Cargo.toml'),'--release','--locked'],check=True)
source=Path(os.environ.get('CARGO_TARGET_DIR',ROOT/'crates/local-correction/target'))/'release/local-correction'
target=ROOT/'src-tauri/resources/local-correction'
target.parent.mkdir(parents=True,exist_ok=True)
shutil.copy2(source,target)
subprocess.run(['codesign','--force','--sign','-',str(target)],check=True)
print('Local correction runtime prepared (no model weights downloaded).')
