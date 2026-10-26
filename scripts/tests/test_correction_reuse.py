import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location('verify_reuse', Path(__file__).resolve().parents[1] / 'verify-correction-reuse.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReuseEvidenceTests(unittest.TestCase):
    def report(self):
        return dict(text='PRIVATE-TEXT', model='PRIVATE-MODEL', backend='cpu', offloaded_layers=0,
                    model_reused=True, elapsed_ms=60,
                    performance=dict(**{key: 0 if key in ('backend_ms', 'model_load_ms') else 10 for key in module.PHASES},
                                     input_tokens=50, generated_tokens=5, peak_rss_bytes=2048))

    def test_evidence_keeps_numbers_and_excludes_body_and_model_metadata(self):
        row = module.sample(self.report(), 90, True)
        self.assertNotIn('PRIVATE', json.dumps(row))
        self.assertEqual(row['model_load_ms'], 0)
        self.assertTrue(row['model_reused'])

    def test_missing_reuse_invalid_metrics_and_empty_output_are_not_success(self):
        for kind in ('reloaded', 'missing', 'string-rss', 'boolean-count', 'empty', 'overlap'):
            report = self.report()
            if kind == 'reloaded': report['performance']['model_load_ms'] = 1
            if kind == 'missing': del report['performance']['prefill_ms']
            if kind == 'string-rss': report['performance']['peak_rss_bytes'] = 'PRIVATE-RSS'
            if kind == 'boolean-count': report['performance']['input_tokens'] = True
            if kind == 'empty': report['text'] = ''
            if kind == 'overlap': report['elapsed_ms'] = 1
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                module.sample(report, 90, True)
