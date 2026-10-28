import importlib.util
import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('macos_support', Path(__file__).resolve().parents[1] / 'verify-macos-support.py')
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)


class MacOSSupportTests(unittest.TestCase):
    def verify(self, host='27.0.1', sdk='27.1', bundle_min='27.0', binary_min='27.0'):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'src-tauri').mkdir()
            (root / 'src-tauri/tauri.conf.json').write_text(json.dumps(
                {'bundle': {'macOS': {'minimumSystemVersion': '27.0'}}}))
            app = root / 'Test.app'
            (app / 'Contents').mkdir(parents=True)
            with (app / 'Contents/Info.plist').open('wb') as target:
                plistlib.dump({'LSMinimumSystemVersion': bundle_min, 'CFBundleExecutable': 'test'}, target)
            report = root / 'report.json'

            def command(args, **kwargs):
                if args[0] == 'sw_vers':
                    return host
                if args[1] == 'vtool':
                    return f'Load command 9\n      cmd LC_BUILD_VERSION\n    minos {binary_min}\n      sdk 27.1\n'
                return sdk

            argv = ['verify', '--app', str(app), '--output', str(report)]
            with patch.object(support, 'ROOT', root), patch('sys.argv', argv), \
                    patch.object(support.subprocess, 'check_output', side_effect=command), patch('builtins.print'):
                try:
                    support.main()
                finally:
                    self.assertTrue(report.exists())
                    result = json.loads(report.read_text())
                    self.assertEqual(result['status'], 'passed' if host.startswith('27') and sdk.startswith('27')
                                     and bundle_min == '27.0' and binary_min == '27.0' else 'failed')
            return result

    def test_supported_app_checks_all_seven_executables(self):
        self.assertEqual(len(self.verify()['binaries']), 7)

    def test_old_host_is_not_runtime_coverage(self):
        with self.assertRaises(ValueError):
            self.verify(host='26.6.2')

    def test_old_sdk_rejected(self):
        with self.assertRaises(ValueError):
            self.verify(sdk='26.6')

    def test_bundle_minimum_must_match(self):
        with self.assertRaises(ValueError):
            self.verify(bundle_min='14.0')

    def test_stale_executable_cannot_pass(self):
        with self.assertRaises(ValueError):
            self.verify(binary_min='14.0')
