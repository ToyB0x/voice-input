import datetime as dt
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('dependency_age', Path(__file__).resolve().parents[1] / 'check-dependency-age.py')
age = importlib.util.module_from_spec(spec)
spec.loader.exec_module(age)


PNPM_POLICY = "minimumReleaseAge: 2880\nminimumReleaseAgeStrict: true\nminimumReleaseAgeIgnoreMissingTime: false\n"


class DependencyAge(unittest.TestCase):
    def setUp(self):
        self.now = dt.datetime(2026, 10, 25, 12, tzinfo=dt.timezone.utc)

    def test_exact_48_hour_boundary_and_missing_timezone(self):
        self.assertTrue(age.cooled('2026-10-23T12:00:00Z', self.now))
        self.assertFalse(age.cooled('2026-10-23T12:00:01Z', self.now))
        for value in [None, '', '2026-10-23T12:00:00']:
            with self.assertRaises(ValueError):
                age.cooled(value, self.now)

    def test_sparse_index_paths(self):
        self.assertEqual(age.index_path('a'), '1/a')
        self.assertEqual(age.index_path('ab'), '2/ab')
        self.assertEqual(age.index_path('syn'), '3/s/syn')
        self.assertEqual(age.index_path('serde_json'), 'se/rd/serde_json')
        with self.assertRaises(ValueError):
            age.index_path('../secret')

    def test_young_missing_and_tampered_transitive_versions_are_rejected(self):
        entry = {'pubtime': '2026-10-23T12:00:00Z', 'cksum': 'expected'}
        self.assertEqual(age.check_package('transitive', '1.0.0', 'expected', entry, self.now)['version'], '1.0.0')
        for invalid in [None, {'cksum': 'expected'}, dict(entry, cksum='tampered'), dict(entry, pubtime='2026-10-24T00:00:00Z')]:
            with self.assertRaises(ValueError):
                age.check_package('transitive', '1.0.0', 'expected', invalid, self.now)

    def test_all_lockfiles_include_target_specific_transitive_packages(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'src-tauri').mkdir()
            (root / 'crates/helper').mkdir(parents=True)
            lock = '''version = 4
[[package]]
name = "local-path"
version = "0.1.0"
[[package]]
name = "target-transitive"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "expected"
'''
            (root / 'src-tauri/Cargo.lock').write_text(lock)
            (root / 'crates/helper/Cargo.lock').write_text(lock)
            packages, locks = age.packages(root)
            self.assertEqual(len(locks), 2)
            self.assertEqual(packages, [('target-transitive', '1.0.0', 'expected')])
            (root / 'crates/helper/Cargo.lock').write_text(lock.replace(age.CRATES_IO, 'git+https://example.com/repository'))
            with self.assertRaisesRegex(ValueError, 'Unsupported dependency source'):
                age.packages(root)

    def test_stale_metadata_is_refreshed_for_a_new_locked_version(self):
        import tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            metadata = age.Metadata(Path(directory))
            responses = [b'{"vers":"1.0.0"}\n', b'{"vers":"1.0.0"}\n{"vers":"1.0.1"}\n']
            with patch.object(metadata, 'get', side_effect=responses) as get:
                self.assertIn('1.0.1', metadata.crate('example', ['1.0.1']))
                self.assertEqual(get.call_args.kwargs, {'refresh': True})

    def test_native_pnpm_bootstrap_companion_must_also_cool(self):
        import json
        import tempfile
        class Registry:
            companion_time = '2026-10-24T12:00:00Z'
            def crate(self, name, versions):
                return {'1.0.0': {'cksum': 'expected', 'pubtime': '2026-10-20T00:00:00Z'}}
            def npm_release(self, name, version):
                if name == 'pnpm':
                    return '2026-10-20T00:00:00Z', {'optionalDependencies': {'@pnpm/native-test': '12.15.0'}}
                return self.companion_time, {}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'src-tauri').mkdir()
            (root / 'package.json').write_text(json.dumps({'packageManager': 'pnpm@12.15.0'}))
            (root / 'pnpm-workspace.yaml').write_text(PNPM_POLICY)
            (root / 'src-tauri/Cargo.lock').write_text('''version = 4
[[package]]
name = "example"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "expected"
''')
            registry = Registry()
            self.assertTrue(any('companion' in error for error in age.check(root, registry, self.now)['errors']))
            registry.companion_time = None
            self.assertTrue(age.check(root, registry, self.now)['errors'])
            registry.companion_time = '2026-10-20T00:00:00Z'
            self.assertEqual(age.check(root, registry, self.now)['errors'], [])

    def test_security_exception_is_exact_checked_and_time_bounded(self):
        checksum = 'a' * 64
        published = '2026-10-25T11:00:00Z'
        entry = {'cksum': checksum, 'pubtime': published}
        approval = dict(ecosystem='crates.io', name='example', version='1.0.1', checksum=checksum,
                        reason='Confirmed upstream security fix', references=['https://example.org/security'],
                        expiresAt='2026-10-27T00:00:00Z')
        policies = {('crates.io', 'example', '1.0.1'): approval}
        record = age.check_package('example', '1.0.1', checksum, entry, self.now, policies)
        self.assertEqual(record['securityException']['reason'], approval['reason'])
        with self.assertRaises(ValueError):  # No exemption for the next release.
            age.check_package('example', '1.0.2', checksum, entry, self.now, policies)
        for invalid in [dict(approval, checksum='b' * 64), dict(approval, expiresAt='2026-10-25T00:00:00Z'), dict(approval, expiresAt='2027-01-01T00:00:00Z')]:
            with self.assertRaises(ValueError):
                age.check_package('example', '1.0.1', checksum, entry, self.now, {('crates.io', 'example', '1.0.1'): invalid})
        for invalid in [dict(entry, cksum='b' * 64), dict(entry, pubtime=None)]:
            with self.assertRaises(ValueError):
                age.check_package('example', '1.0.1', checksum, invalid, self.now, policies)

    def test_security_policy_requires_evidence_and_rejects_version_ranges(self):
        import tempfile
        import json
        approval = dict(ecosystem='crates.io', name='example', version='1.0.1', checksum='a' * 64,
                        reason='Upstream security fix', references=['https://example.org/security'],
                        expiresAt='2026-10-27T00:00:00Z')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'dependency-security-exceptions.json'
            path.write_text(json.dumps(dict(schemaVersion=1, exceptions=[approval])))
            self.assertIn(('crates.io', 'example', '1.0.1'), age.security_exceptions(root))
            for invalid in [dict(approval, version='*'), dict(approval, version='^1.0'), dict(approval, reason=''), dict(approval, references=[]), dict(approval, checksum='')]:
                path.write_text(json.dumps(dict(schemaVersion=1, exceptions=[invalid])))
                with self.assertRaises(ValueError):
                    age.security_exceptions(root)

    def test_native_pnpm_cannot_silently_exclude_whole_packages(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'pnpm-workspace.yaml'
            policies = {('npm', 'example', '1.0.1'): {}}
            path.write_text(PNPM_POLICY + 'minimumReleaseAgeExclude: ["example@1.0.1"]\n')
            age.validate_pnpm_exclusions(root, policies)
            for exclusions in ['["example"]', '["*"]', '["example@1.0.2"]', '[]']:
                path.write_text(PNPM_POLICY + 'minimumReleaseAgeExclude: '+exclusions+'\n')
                with self.assertRaises(ValueError):
                    age.validate_pnpm_exclusions(root, policies)
            path.write_text(PNPM_POLICY.replace('2880', '0'))
            with self.assertRaises(ValueError):
                age.validate_pnpm_exclusions(root, {})
