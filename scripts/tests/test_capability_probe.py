import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location('capability_probe', Path(__file__).resolve().parents[1]/'probe-correction-model.py')
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class CapabilityEvidenceTests(unittest.TestCase):
    def test_timeout_preserves_prior_results_continues_and_records_metal(self):
        case = dict(transcript='会議は金曜日です', context='', required=['金曜'])
        ok = dict(wall_ms=1, peak_rss_bytes=100, failed=False, rejected=False,
                  text=case['transcript'], model_report={})
        missing_gpu = dict(wall_ms=1, peak_rss_bytes=100, failed=True, rejected=False,
                           text=case['transcript'], model_report=None,
                           failure=dict(error='runtime', stage='backend', reason='metal-simd-unavailable',
                                        requested_backend='metal', offloaded_layers=0))
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            calls = 0

            def fake_infer(*args):
                nonlocal calls
                calls += 1
                saved = json.loads((output/'report.json').read_text())
                if calls == 1:
                    return ok
                if calls == 2:
                    self.assertEqual([r['id'] for r in saved['cases']], ['first'])
                    raise subprocess.TimeoutExpired('synthetic', 180)
                self.assertEqual(saved['cases'][1]['failure']['stage'], 'subprocess-timeout')
                return missing_gpu

            with patch.object(probe, 'infer', side_effect=fake_infer):
                with self.assertRaisesRegex(RuntimeError, 'Model execution failed'):
                    probe.run_probes(Path('binary'), Path('model'),
                                     [dict(id='first', **case), dict(id='timeout', **case)],
                                     output, 'synthetic', {})
            saved = json.loads((output/'report.json').read_text())
            self.assertEqual(len(saved['cases']), 2)
            self.assertTrue(saved['metal_unavailable'])
            self.assertTrue(saved['cases'][1]['failed'])
            self.assertFalse(saved['cases'][1]['rejected'])
            self.assertIn('UNAVAILABLE', (output/'summary.md').read_text())
