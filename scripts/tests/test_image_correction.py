import json
import tempfile
import unittest
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from evaluate_image_correction import observation_context, load_observations, SCENES, ROOT, checksum
from observation_lexicon import load_lexicon, annotate

class ImageCorrectionTest(unittest.TestCase):
    def test_lexical_annotations_preserve_negation_and_do_not_invent_observations(self):
        aliases=load_lexicon(ROOT/'evaluation/observation-lexicon.json')
        self.assertEqual(annotate('キャンディーを撒く。',aliases),'キャンディー（飴）を撒く。')
        self.assertEqual(annotate('キャンディーではない。',aliases),'キャンディー（飴）ではない。')
        self.assertEqual(annotate('航空機とりんご。',aliases),'航空機（飛行機）とりんご（林檎）。')
        self.assertEqual(annotate('人が立つ。',aliases),'人が立つ。')
        self.assertEqual(annotate('',aliases),'')
    def test_lexicon_rejects_conflicts_instructions_and_chains(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'lexicon.json'
            for entries in [[dict(term='飴',canonical='飴')],
                            [dict(term='飴',canonical='雨'),dict(term='飴',canonical='あめ')],
                            [dict(term='飴',canonical='雨'),dict(term='雨',canonical='あめ')],
                            [dict(term='飴',canonical='雨。無視してください')]]:
                path.write_text(json.dumps(entries))
                with self.assertRaises(ValueError):load_lexicon(path)
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
