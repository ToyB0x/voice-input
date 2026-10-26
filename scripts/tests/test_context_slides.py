import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('context_slides',Path(__file__).resolve().parents[1]/'context-slides.py')
slides=importlib.util.module_from_spec(spec);spec.loader.exec_module(slides)

class ConclusionsTests(unittest.TestCase):
    def test_new_success_and_new_omissions_are_read_from_the_supplied_report(self):
        row=dict(scene='candy',source='vision-labels',engine='qwen3',instruction='kanji',route='both',judgment='undetermined')
        self.assertEqual(slides.condition_counts([row],'candy','vision-labels')['target'],0)
        row['judgment']='target'
        self.assertEqual(slides.condition_counts([row],'candy','vision-labels')['target'],1)
        row['source']='ocr'
        self.assertEqual(slides.condition_counts([row],'candy','vision-labels')['executed'],0)
        control=dict(input='山田さんへ資料を送ります',instruction='default',result=dict(accepted=True,candidate='資料を送ります。'))
        self.assertEqual(slides.omitted_fragments([control],'default'),['山田さんへ'])
        self.assertEqual(slides.omitted_fragments([control],'kanji'),[])
        control['result']['accepted']=False
        self.assertEqual(slides.omitted_fragments([control],'default'),[])
