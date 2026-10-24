import json,sys,unittest
from unittest.mock import patch
from subprocess import CompletedProcess
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from evaluate_correction import constraints,MODES,infer,metal_unavailable
class CorrectionTests(unittest.TestCase):
    def test_four_routes_change_destinations_independently(self):
        self.assertEqual(set(MODES.values()),{(False,False),(True,False),(False,True),(True,True)})
    def test_constraints_expose_fact_loss_invention_and_missing_format(self):
        result=constraints('予約します。予算150万円。',dict(required=['しません','120万円'],forbidden=['150万円'],line_break=True))
        self.assertEqual(result['missing'],['しません','120万円'])
        self.assertEqual(result['invented'],['150万円'])
        self.assertFalse(result['format_ok'])
    def test_runtime_failure_is_distinct_from_protocol_rejection(self):
        for kind,rejected in [('runtime',False),('rejected',True)]:
            result=CompletedProcess([],1,json.dumps(dict(error=kind,stage='validation' if rejected else 'model')),'')
            with patch('evaluate_correction.subprocess.run',return_value=result):
                item=infer(Path('/binary'),Path('/model'),'cpu','original','')
            self.assertTrue(item['failed'])
            self.assertEqual(item['rejected'],rejected)
            self.assertEqual(item['text'],'original')
    def test_only_explicit_missing_hardware_is_unavailable(self):
        failure=dict(error='runtime',stage='backend',reason='metal-simd-unavailable',requested_backend='metal',offloaded_layers=0)
        self.assertTrue(metal_unavailable(failure))
        for update in [dict(reason='metal-backend-unavailable'),dict(stage='generation'),dict(offloaded_layers=1),dict(requested_backend='cpu')]:
            self.assertFalse(metal_unavailable(dict(failure,**update)))
