import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('context_slides',Path(__file__).resolve().parents[1]/'context-slides.py')
slides=importlib.util.module_from_spec(spec);spec.loader.exec_module(slides)

class ConclusionsTests(unittest.TestCase):
    def test_new_success_and_new_omissions_are_read_from_the_supplied_report(self):
        row=dict(scene='candy',source='vision-labels',engine='qwen3',instruction='kanji',route='both',judgment='undetermined',status='accepted',finalText='飴')
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

    def test_afm_custom_text_only_reports_do_not_use_qwen_or_unavailable_rows(self):
        row=dict(scene='storm',source='ocr',engine='apple-foundation',instruction='custom',route='correction',status='accepted',finalText='雨',judgment='target')
        self.assertEqual(slides.condition_counts([row],'storm','ocr',engine='apple-foundation',instruction='custom',route='correction')['target'],1)
        self.assertEqual(slides.condition_counts([row],'storm','ocr')['executed'],0)
        row['status']='unavailable'
        self.assertEqual(slides.condition_counts([row],'storm','ocr',engine='apple-foundation',instruction='custom',route='correction')['executed'],0)

    def test_afm_text_only_html_uses_custom_route_and_no_legacy_qwen_controls(self):
        import json, tempfile
        from unittest.mock import patch
        report=json.loads((Path(__file__).resolve().parents[2]/'evaluation/context-evidence/baseline/results.json').read_text())
        report.update(inputKind='text',presentationEngine='apple-foundation',presentationInstruction='custom',networkDenied=False,instructions={'default':'','custom':'表記の希望'})
        report['models']['speech']='なし';report['models']['speechBackend']='none'
        report['rows']=[dict(scene='storm',source='fixture-text',engine='apple-foundation',instruction='custom',route='correction',repeat=0,status='accepted',raw='あめ',finalText='雨',judgment='target',correctionMs=100)]
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'device.json';source.write_text(json.dumps(report))
            with patch('sys.argv',['slides','--report',str(source),'--output',str(root/'output')]): slides.main()
            html=(root/'output/slides.html').read_text()
            self.assertIn('台風 1/1',html)
            self.assertIn('ローカルモデルAPI（通信Sandboxなし）',html)
            self.assertNotIn('田中さんへ資料を送ります',html)
            report['rows'][0]['status']='unavailable';report['rows'][0]['correctionMs']=None
            source.write_text(json.dumps(report))
            with patch('sys.argv',['slides','--report',str(source),'--output',str(root/'output')]): slides.main()
            self.assertIn('未実行',(root/'output/slides.html').read_text())
