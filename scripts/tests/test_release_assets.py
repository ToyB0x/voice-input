import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('release_assets',Path(__file__).resolve().parents[1]/'release-assets.py')
release=importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

class Releases(unittest.TestCase):
    def test_tags_are_safe_and_stable(self):
        self.assertEqual(release.version('v1.2.3'),'1.2.3')
        self.assertEqual(release.version('v0.0.1'),'0.0.1')
        for bad in ['1.2.3','v01.2.3','v1.2.3-rc1','v1.2.3/../../main','$(pwd)']:
            with self.assertRaises(ValueError):release.version(bad)

    def test_native_assets_and_manifest_are_consistent(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);bundle=root/'bundle';bundle.mkdir()
            (bundle/'Voice Input.app.tar.gz').write_bytes(b'fixture archive')
            (bundle/'Voice Input.app.tar.gz.sig').write_text('fixture signature\n')
            (bundle/'Voice Input.dmg').write_bytes(b'fixture installer')
            output=root/'release'
            release.package(bundle,output,'v1.2.3')
            manifest=json.loads((output/'latest.json').read_text())
            self.assertEqual(manifest['version'],'1.2.3')
            entry=manifest['platforms']['darwin-aarch64']
            self.assertEqual(entry['url'],'https://github.com/ToyB0x/voice-input/releases/download/v1.2.3/Voice-Input_1.2.3_aarch64.app.tar.gz')
            self.assertEqual(entry['signature'],'fixture signature')
            self.assertEqual(len(list(output.iterdir())),5)
            with self.assertRaises(FileExistsError):release.package(bundle,output,'v1.2.3')

    def test_version_updates_only_the_app(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);(root/'src-tauri').mkdir()
            for name in ['package.json','src-tauri/tauri.conf.json']:(root/name).write_text('{"version":"0.1.0"}')
            (root/'pnpm-lock.yaml').write_text("lockfileVersion: '9.0'\n")
            (root/'src-tauri/Cargo.toml').write_text('[package]\nname = "voice-input"\nversion = "0.1.0"\n')
            (root/'src-tauri/Cargo.lock').write_text('[[package]]\nname = "other"\nversion = "0.1.0"\n[[package]]\nname = "voice-input"\nversion = "0.1.0"\n')
            release.set_version(root,'v2.3.4')
            self.assertEqual(json.loads((root/'package.json').read_text())['version'],'2.3.4')
            self.assertEqual((root/'pnpm-lock.yaml').read_text(),"lockfileVersion: '9.0'\n")
            self.assertIn('name = "other"\nversion = "0.1.0"',(root/'src-tauri/Cargo.lock').read_text())
            self.assertIn('name = "voice-input"\nversion = "2.3.4"',(root/'src-tauri/Cargo.lock').read_text())
