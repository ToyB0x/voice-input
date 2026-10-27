import json
import tempfile
import unittest
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from evaluate_image_correction import observation_context, load_observations, SCENES, ROOT, checksum

class ImageCorrectionTest(unittest.TestCase):
    def test_only_asserted_observations_are_forwarded(self):
        row={'scene':'SECRET_SCENE','raw':'SECRET_RAW','observation':dict(objects=['人'],actions=['立つ'],visible_text=[],uncertain=['SECRET_GUESS'])}
        text=observation_context(row)
        self.assertEqual(text,'物体：人。\n動作：立つ。')
        self.assertNotIn('SECRET',text)
    def test_malformed_observation_is_not_silently_empty(self):
        for obs in [dict(objects=[]),dict(objects='人',actions=[],visible_text=[],uncertain=[]),dict(objects=['a'*41],actions=[],visible_text=[],uncertain=[])]:
            with self.assertRaises(ValueError):observation_context({'observation':obs})
    def test_incomplete_duplicate_and_wrong_image_reports_are_rejected(self):
        rows=[dict(scene=scene,repeat=repeat,status='completed',image_sha256=checksum(ROOT/'evaluation/context-evidence/assets'/f'{scene}.png'),
              observation=dict(objects=['人'],actions=[],visible_text=[],uncertain=[])) for scene in SCENES for repeat in [0,1]]
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'report.json'
            report=dict(complete=True,runtime_pass=True,rows=rows)
            path.write_text(json.dumps(report)); self.assertEqual(len(load_observations(path)['rows']),8)
            report['runtime_pass']=False;path.write_text(json.dumps(report))
            with self.assertRaises(ValueError):load_observations(path)
            report['runtime_pass']=True;rows[0]['image_sha256']='wrong';path.write_text(json.dumps(report))
            with self.assertRaises(ValueError):load_observations(path)
            rows[0]=rows[1];path.write_text(json.dumps(report))
            with self.assertRaises(ValueError):load_observations(path)
if __name__=='__main__':unittest.main()
