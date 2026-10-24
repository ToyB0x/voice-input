import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from evaluate_correction import constraints,MODES
class CorrectionTests(unittest.TestCase):
    def test_four_routes_change_destinations_independently(self):
        self.assertEqual(set(MODES.values()),{(False,False),(True,False),(False,True),(True,True)})
    def test_constraints_expose_fact_loss_invention_and_missing_format(self):
        result=constraints('予約します。予算150万円。',dict(required=['しません','120万円'],forbidden=['150万円'],line_break=True))
        self.assertEqual(result['missing'],['しません','120万円'])
        self.assertEqual(result['invented'],['150万円'])
        self.assertFalse(result['format_ok'])
