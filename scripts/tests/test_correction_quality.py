import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from correction_quality import constraints, quality_summary, load_baseline, compare_baseline
from correction_models import MODELS
from transcription_models import MODELS as ASR_MODELS, checksum
from evaluate_models import ROOT, normalize, score
from evaluate_correction import MODES, load_asr, route_summary, measured_infer, persist_report, route_requests, host_info
from summarize_correction import EXPECTED, validate_reports


def result(case, text, failed=False, rejected=False):
    return dict(case=case, text=text, failed=failed, rejected=rejected,
                constraints=constraints(text, case), wall_ms=1, peak_rss_bytes=1024)


class FixedQualityTests(unittest.TestCase):
    def test_fact_loss_and_format_failures_are_independent(self):
        case = dict(required=['120万円'], forbidden=['150万円'], punctuation=True)
        self.assertFalse(constraints('150万円。', case)['facts_ok'])
        self.assertTrue(constraints('150万円。', case)['format_ok'])
        self.assertTrue(constraints('120万円', case)['facts_ok'])
        self.assertFalse(constraints('120万円', case)['format_ok'])
        self.assertFalse(constraints('資料\n予約', dict(bullets=2))['format_ok'])
        self.assertTrue(constraints('- 資料\n- 予約', dict(bullets=2))['format_ok'])
        self.assertFalse(constraints('結論\n理由', dict(paragraphs=2))['format_ok'])
        self.assertTrue(constraints('結論\n\n理由', dict(paragraphs=2))['format_ok'])

    def test_rejected_and_failed_fallbacks_are_not_generated_successes(self):
        case = dict(transcript='会議', required=['会議'])
        rows = [result(case, '会議'), result(case, '会議', True, True),
                result(case, '会議', True), result(dict(transcript=''), '')]
        q = quality_summary(rows)
        self.assertEqual((q['generated'], q['facts_passed'], q['format_passed']), (1, 1, 1))
        self.assertEqual((q['rejected'], q['runtime_failures']), (1, 1))

    def test_baseline_requires_identical_inputs_and_rescores_same_cases(self):
        case = dict(id='old', transcript='えー会議', context='', reference='会議',
                    required=['会議'], forbidden_format=['えー'])
        saved = dict(schema_version=1, model='synthetic', model_identity={},
                     source_revision='old', run_url='public-run',
                     cases=[dict(case={k:case[k] for k in ['id','transcript','context','reference']},
                                 result=dict(text='えー会議', failed=False, rejected=False))])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'baseline.json'
            path.write_text(json.dumps(saved))
            baseline = load_baseline(path, 'synthetic', {}, [case])
            comparison = compare_baseline(baseline, [result(case, '会議'), result(dict(id='new', transcript='新規'), '新規')])
            self.assertEqual(comparison['before']['generated'], 1)
            self.assertEqual(comparison['after']['generated'], 1)
            self.assertEqual(comparison['before']['format_passed'], 0)
            self.assertEqual(comparison['after']['format_passed'], 1)
            self.assertIn('-えー会議', comparison['pairs'][0]['diff'])
            with self.assertRaises(ValueError):
                load_baseline(path, 'synthetic', {}, [dict(case, context='changed')])
            with self.assertRaises(ValueError):
                load_baseline(path, 'wrong-model', {}, [case])

    def test_partial_report_survives_timeout_and_has_no_fake_zero_cer(self):
        with patch('evaluate_correction.infer', side_effect=subprocess.TimeoutExpired('synthetic', 180)):
            item = measured_infer(Path('binary'), Path('model'), 'cpu', '原文', '')
        self.assertEqual(item['text'], '原文')
        self.assertTrue(item['failed'])
        self.assertFalse(item['rejected'])
        report = dict(complete=False, records=[], supplemental=[], metal_smoke=None,
                      metal_skipped=True, metal_unavailable=False, limitation='synthetic', baseline=None)
        with tempfile.TemporaryDirectory() as directory:
            persist_report(Path(directory), report)
            saved = json.loads((Path(directory)/'report.json').read_text())
            self.assertIsNone(saved['summary']['none']['cer'])
            self.assertFalse(saved['complete'])
            self.assertIn('pending', (Path(directory)/'summary.md').read_text())


class ProvenanceTests(unittest.TestCase):
    def test_macos_memory_metadata_uses_sysctl_instead_of_linux_sysconf(self):
        with patch('evaluate_correction.platform.system', return_value='Darwin'), patch('evaluate_correction.subprocess.check_output', return_value='7516192768\n'), patch('evaluate_correction.os.sysconf', side_effect=AssertionError('Linux-only selector')):
            self.assertEqual(host_info()['physical_memory_bytes'], 7516192768)

    def test_requests_freeze_asr_and_context_independently(self):
        corpus=dict(cases=[dict(id='one', reference='正解', hint='固定画面情報')])
        asr={mode:dict(cases=[dict(observations=[dict(text=text), dict(text=text)])])
             for mode,text in [('none','元の認識'),('related','ヒント付きの認識')]}
        rows=list(route_requests(corpus,asr))
        self.assertEqual([(r['mode'],r['raw_asr'],r['context']) for r in rows],
                         [('none','元の認識',''),('asr_only','ヒント付きの認識',''),
                          ('correction_only','元の認識','固定画面情報'),('both','ヒント付きの認識','固定画面情報')])

    def test_stale_or_edited_asr_inputs_are_rejected_before_inference(self):
        corpus = dict(cases=[dict(id='a')])
        raw = dict(model_name='small', backend='cpu', cases=[dict(id='a', observations=[dict(text='会議'), dict(text='会議')])])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for mode in ['none','related']:
                (root/(mode+'.json')).write_text(json.dumps(raw))
            metadata = dict(protocol_version='context-evaluation-v2', model='small', backend='cpu',
                            model_sha256=ASR_MODELS['small']['sha256'], corpus_sha256=checksum(ROOT/'evaluation/corpus.json'),
                            source_revision='revision', network_denied=True,
                            binary_sha256='a'*64,
                            asr_sha256={mode:checksum(root/(mode+'.json')) for mode in ['none','related']})
            path = root/'comparison.json'
            path.write_text(json.dumps(metadata))
            load_asr(root, corpus, 'revision', True)
            for update in [dict(source_revision='old'), dict(network_denied=False), dict(corpus_sha256='old')]:
                path.write_text(json.dumps(dict(metadata, **update)))
                with self.assertRaises(ValueError):
                    load_asr(root, corpus, 'revision', True)
            path.write_text(json.dumps(metadata))
            (root/'none.json').write_text(json.dumps(dict(raw, model_name='base')))
            with self.assertRaises(ValueError):
                load_asr(root, corpus, 'revision', True)


    def test_turbo_engine_name_requires_pinned_turbo_identity(self):
        corpus=dict(cases=[dict(id='a')])
        raw=dict(model_name='large',backend='cpu',cases=[dict(id='a',observations=[dict(text='会議'),dict(text='会議')])])
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for mode in ['none','related']:(root/(mode+'.json')).write_text(json.dumps(raw))
            metadata=dict(protocol_version='context-evaluation-v2',model='large-v3-turbo',backend='cpu',
                          model_sha256=ASR_MODELS['large-v3-turbo']['sha256'],corpus_sha256=checksum(ROOT/'evaluation/corpus.json'),
                          source_revision='revision',network_denied=True,binary_sha256='a'*64,
                          asr_sha256={mode:checksum(root/(mode+'.json')) for mode in ['none','related']})
            (root/'comparison.json').write_text(json.dumps(metadata))
            load_asr(root,corpus,'revision',True,'large-v3-turbo')
            with self.assertRaises(ValueError):load_asr(root,corpus,'revision',True,'medium')
            metadata['model_sha256']=ASR_MODELS['large-v3']['sha256']
            (root/'comparison.json').write_text(json.dumps(metadata))
            with self.assertRaises(ValueError):load_asr(root,corpus,'revision',True,'large-v3-turbo')


class ComparableReportTests(unittest.TestCase):
    def setUp(self):
        self.corpus = dict(cases=[dict(id='spoken', reference='会議', hint='会議')])
        self.cases = [dict(id='fixed', transcript='会議', context='', reference='会議', required=['会議'])]
        self.reports = []
        for name in EXPECTED:
            rows=[]
            for mode, (_, context) in MODES.items():
                rows.append(dict(id='spoken', mode=mode, raw_asr='会議', text='会議', reference='会議',
                                 context='会議' if context else '', raw_score=score(normalize('会議'),normalize('会議')),
                                 final_score=score(normalize('会議'),normalize('会議')), failed=False, rejected=False,
                                 wall_ms=1, peak_rss_bytes=1024))
            supplemental=[result(self.cases[0], '会議')]
            self.reports.append(dict(protocol_version='correction-quality-v2', complete=True, model=name,
                model_identity=MODELS[name], source_revision='revision', source_dirty=False, network_denied=True, backend='cpu',
                metal_skipped=True, metal_smoke=None, host=dict(system='Linux', machine='x86_64', logical_cpus=4),
                asr_input_sha256=dict(none='a', related='b'), asr_binary_sha256='c', corpus_sha256='d', supplemental_sha256='e',
                runtime_source_sha256='f', cargo_lock_sha256='g', expected_records=4, expected_supplemental=1,
                records=rows, supplemental=supplemental, summary=route_summary(rows), quality=quality_summary(supplemental)))

    def test_preset_comparison_uses_distinct_verified_asr_models(self):
        from summarize_presets import validate
        reports=[]
        for asr in ['medium','large-v3-turbo']:
            report=copy.deepcopy(self.reports[1])
            report.update(asr_model=asr,asr_model_sha256=ASR_MODELS[asr]['sha256'],asr_input_sha256={'none':asr,'related':asr+'hint'})
            reports.append(report)
        self.assertEqual(set(validate(reports,self.corpus,self.cases,'revision')),{'medium','large-v3-turbo'})
        for mutate in [lambda r:r.pop(),lambda r:r[0].update(asr_model_sha256='wrong'),
                       lambda r:r[0].update(model='qwen3-8b'),lambda r:r[0]['records'].pop()]:
            invalid=copy.deepcopy(reports);mutate(invalid)
            with self.assertRaises(ValueError):validate(invalid,self.corpus,self.cases,'revision')

    def test_complete_matrix_is_accepted(self):
        self.assertEqual(set(validate_reports(self.reports, self.corpus, self.cases, 'revision')), set(EXPECTED))

    def test_missing_cases_offline_evidence_and_mixed_inputs_are_rejected(self):
        for mutate in [lambda r:r.pop(), lambda r:r[0]['records'].pop(),
                       lambda r:r[0].update(network_denied=False),
                       lambda r:r[0].update(complete=False),
                       lambda r:r[0].update(asr_input_sha256={'none':'other','related':'b'})]:
            reports=copy.deepcopy(self.reports)
            mutate(reports)
            with self.assertRaises(ValueError):
                validate_reports(reports, self.corpus, self.cases, 'revision')

    def test_forged_labels_are_rejected_even_when_totals_agree(self):
        reports=copy.deepcopy(self.reports)
        reports[0]['supplemental'][0]['constraints']['facts_ok']=False
        reports[0]['quality']=quality_summary(reports[0]['supplemental'])
        with self.assertRaises(ValueError):
            validate_reports(reports, self.corpus, self.cases, 'revision')
