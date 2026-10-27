import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from evaluate_vlm import parse_observation, PROMPT

class ObservationTest(unittest.TestCase):
    def test_bounded_schema_and_no_reference_leakage(self):
        self.assertEqual(parse_observation('{"objects":["人"],"actions":[],"visible_text":[],"uncertain":[]}')['objects'],['人'])
        for word in ['雨','飴','candy','rainfall','あめ']:
            self.assertNotIn(word,PROMPT)
        for text in ['{"objects":[]}', '{"objects":"人","actions":[],"visible_text":[],"uncertain":[]}',
                     '説明 {"objects":[],"actions":[],"visible_text":[],"uncertain":[]}']:
            with self.assertRaises(ValueError):parse_observation(text)
if __name__=='__main__':unittest.main()
