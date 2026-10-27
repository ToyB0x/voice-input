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
            report['kanaControls']=[dict(engine='apple-foundation',instruction='custom',input='あめ',result=dict(candidate=None,accepted=None,status='unavailable'))]
            report['preservationControls']=[dict(engine='apple-foundation',instruction='custom',input='原文',result=dict(candidate=None,accepted=None,status='unavailable'))]
            source.write_text(json.dumps(report))
            with patch('sys.argv',['slides','--report',str(source),'--output',str(root/'output')]): slides.main()
            self.assertIn('未実行',(root/'output/slides.html').read_text())
            report['rows']=[dict(scene='storm',source='none',engine='none',instruction='none',route='none',repeat=0,status='error',raw=None,finalText=None)]
            source.write_text(json.dumps(report))
            with patch('sys.argv',['slides','--report',str(source),'--output',str(root/'output')]): slides.main()
            self.assertIn('未取得／失敗',(root/'output/slides.html').read_text())

    def test_device_snapshot_matches_original_diagnostics_and_keeps_interpretation_scoped(self):
        import json, hashlib, tempfile
        from unittest.mock import patch
        root=Path(__file__).resolve().parents[2]/'evaluation/context-evidence/device-afm-v0.0.2'
        report=json.loads((root/'results.json').read_text())
        original=json.loads((root/'diagnostic.json').read_text())
        proof=json.loads((root/'provenance.json').read_text())
        self.assertEqual(hashlib.sha256((root/'results.json').read_bytes()).hexdigest(),proof['reportSha256'])
        self.assertEqual(hashlib.sha256((root/'diagnostic.json').read_bytes()).hexdigest(),proof['originalDiagnosticSha256'])
        self.assertEqual(report['reportId'],original['reportId'])
        self.assertEqual(len(report['rows']),original['totalRows'])
        for key in ['appVersion','binaries','sourceRevision','foundation','models','fixture']:
            self.assertEqual(report[key],original[key])
        text_keys={'raw','candidate','finalText','context','candidateExcerpt','candidateTruncated'}
        for row,before in zip(report['rows'],original['rows']):
            self.assertEqual({k:v for k,v in row.items() if k not in text_keys},{k:v for k,v in before.items() if k not in text_keys})
            if row.get('candidateTruncated'):
                self.assertIsNone(row['candidate'])
                self.assertTrue(row['candidateExcerpt'])
            if row.get('finalText') is not None:
                expected=row['expected'] in row['finalText'];opposite=row['opposite'] in row['finalText']
                judgment='target' if expected and not opposite else 'opposite' if opposite and not expected else 'undetermined'
                self.assertEqual(row['judgment'],judgment)
        with tempfile.TemporaryDirectory() as temp:
            with patch('sys.argv',['slides','--report',str(root/'results.json'),'--output',temp]): slides.main()
            html=(Path(temp)/'slides.html').read_text()
            self.assertIn('音声・Whisperなし',html)
            self.assertNotIn('文脈なしのWhisper原文',html)
            self.assertNotIn('全候補・棄却・時間は同梱',html)
            self.assertIn('元の全文JSONや省略部分を再構成したものではない',html)
            self.assertIn('期待語の一致と、意味・文体の保持は別',html)
            self.assertIn('長い候補の抜粋 2件',html)

    def test_falling_slides_select_one_input_form_and_keep_real_scene_names(self):
        import json,tempfile
        from unittest.mock import patch
        report=json.loads((Path(__file__).resolve().parents[2]/'evaluation/context-evidence/baseline/results.json').read_text())
        report.update(experiment={'dataset':'ame-falling-v1'},inputKind='text',presentationEngine='apple-foundation',presentationInstruction='custom',instructions={'custom':''})
        report['fixture']['utterance']='あめがふっています'
        report['captures']={('rainfall' if k=='storm' else 'candy-shower'):v for k,v in report['captures'].items()}
        report['rows']=[dict(scene='candy-shower',source='vision-labels',engine='apple-foundation',instruction='custom',route='correction',repeat=0,status='accepted',raw='雨が降っています',finalText=text,judgment=judgment,correctionMs=100,inputForm=form,finalPass=passed) for form,text,judgment,passed in [('original','雨が降っています','opposite',False),('reading','飴が降っています','target',True)]]
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'synthetic-test-only.json';source.write_text(json.dumps(report))
            with patch('sys.argv',['slides','--report',str(source),'--output',str(root/'out'),'--input-form','reading']): slides.main()
            html=(root/'out/slides.html').read_text()
            self.assertTrue('屋根からの飴まき' in html)
            self.assertTrue('最終試験：語尾を含む文全体の一致' in html)
            self.assertTrue('飴まき 1/1' in html)
            self.assertTrue('かな原文' in html)
