import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from measure_correction import PHASES, numeric_sample, write_results


class PerformanceTests(unittest.TestCase):
    def sample(self):
        return dict(failed=False, wall_ms=90.5, peak_rss_bytes=2048,
                    model_report=dict(text='PRIVATE-TEXT', model='PRIVATE-NAME', elapsed_ms=60,
                                      performance=dict(**{key: 10 for key in PHASES},
                                                       input_tokens=50, generated_tokens=5, peak_rss_bytes=2048)))

    def test_rejection_failure_or_missing_measurement_cannot_be_a_speedup(self):
        for change in ('failure', 'missing', 'negative', 'bool', 'overlap', 'no-inference'):
            item = self.sample()
            if change == 'failure': item['failed'] = True
            if change == 'missing': del item['model_report']['performance']['prefill_ms']
            if change == 'negative': item['model_report']['performance']['prefill_ms'] = -1
            if change == 'bool': item['model_report']['performance']['input_tokens'] = True
            if change == 'overlap': item['model_report']['elapsed_ms'] = 59
            if change == 'no-inference': item['model_report']['performance']['generated_tokens'] = 0
            with self.subTest(change=change), self.assertRaises(ValueError):
                numeric_sample(item, 1)

    def test_saved_metrics_exclude_body_and_distinguish_fresh_processes(self):
        item = self.sample()
        item['diagnostic_stderr'] = 'PRIVATE-STDERR'
        row = numeric_sample(item, 2)
        self.assertFalse(row['process_reused'])
        self.assertEqual(row['invocation'], 2)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)
            write_results(path, dict(model='qwen3-1.7b', backend='cpu', network_denied=True, samples=[row]))
            for file in path.iterdir():
                self.assertNotIn('PRIVATE', file.read_text())
            self.assertEqual(json.loads((path / 'performance.json').read_text())['samples'][0], row)
