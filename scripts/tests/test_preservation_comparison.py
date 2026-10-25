import copy
import json
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from correction_baselines import compare_routes,load_routes,validate_frozen
from correction_quality import constraints,quality_summary,load_cases,load_baseline,compare_baseline
from evaluate_correction import MODES
from evaluate_guard_regression import metrics
from evaluate_models import ROOT
from correction_models import MODELS

class PreservationComparisonTests(unittest.TestCase):
    def test_guard_accepts_and_false_rejections_are_separate(self):
        cases=[dict(id='good',should_accept=True),dict(id='bad',should_accept=False)]
        result=metrics(cases,[dict(id='good',accepted=False),dict(id='bad',accepted=True)])
        self.assertEqual((result['false_rejections'],result['false_accepts']),(1,1))
        with self.assertRaises(ValueError):metrics(cases,[dict(id='bad',accepted=False)])
        policy=metrics([dict(id='policy',should_accept=False,category='conservative-repair')],[dict(id='policy',accepted=False)])
        self.assertEqual((policy['harmful_cases'],policy['policy_cases'],policy['policy_rejected']),(0,1,1))

    def test_alternative_wording_is_declared_and_missing_is_counted(self):
        case=dict(required=['木曜'],required_any=[['水曜ではなく','水曜じゃなく']])
        self.assertTrue(constraints('水曜じゃなく木曜です',case)['facts_ok'])
        self.assertFalse(constraints('木曜です',case)['facts_ok'])
        before=dict(case=dict(case,transcript='水曜じゃなく木曜です'),text='木曜です',failed=False,rejected=False)
        before['constraints']=constraints(before['text'],case)
        after=dict(before,text='水曜じゃなく木曜です',failed=True,rejected=True)
        after['constraints']=constraints(after['text'],case)
        b,a=quality_summary([before]),quality_summary([after])
        self.assertEqual((b['generated'],b['final_facts_passed'],b['missing_required_cases']),(1,0,1))
        self.assertEqual((a['generated'],a['final_facts_passed'],a['missing_required_cases']),(0,1,0))

    def test_route_comparison_pairs_only_same_requests_and_counts_fallback(self):
        rows=[dict(id=id,mode=mode,raw_asr='また会議です',reference='また会議です',context='',
                   result=dict(text='会議です',failed=False,rejected=False)) for id in ['one','two'] for mode in MODES]
        group=dict(asr_model='small',source_revision='old',run_url='public',records=rows)
        current=[dict(**{k:v for k,v in row.items() if k!='result'},text='また会議です',failed=True,rejected=True) for row in rows]
        result=compare_routes(group,current)
        self.assertEqual(result['matched'],8)
        self.assertGreater(result['summary']['none']['before']['cer'],0)
        self.assertEqual(result['summary']['none']['after']['cer'],0)
        self.assertEqual(result['summary']['none']['after']['rejected'],2)
        current[0]['raw_asr']='changed'
        result=compare_routes(group,current)
        self.assertEqual((result['matched'],len(result['skipped_inputs'])),(7,1))
        current[4]['raw_asr']='also changed'
        with self.assertRaises(ValueError):compare_routes(group,current)
        with self.assertRaises(ValueError):compare_routes(group,current[:-1])

    def test_frozen_routes_reject_altered_reference_context_and_identity(self):
        baseline=json.loads((ROOT/'evaluation/correction-baseline/qwen3-4b-preservation.json').read_text())
        corpus=json.loads((ROOT/'evaluation/corpus.json').read_text())
        for model in ['small','medium','large-v3-turbo']:
            self.assertEqual(len(load_routes(baseline,model,corpus,MODES)['records']),32)
        for field in ['reference','context']:
            changed=copy.deepcopy(baseline);changed['routes']['small']['records'][0][field]='altered'
            with self.assertRaises(ValueError):load_routes(changed,'small',corpus,MODES)
        changed=copy.deepcopy(baseline);changed['routes']['small']['model_sha256']='altered'
        with self.assertRaises(ValueError):load_routes(changed,'small',corpus,MODES)
        cases=load_cases(ROOT/'evaluation/correction-cases.json')
        with self.assertRaises(ValueError):validate_frozen(dict(model='qwen3-4b',asr_model='small'),corpus,cases,MODES)

    def test_frozen_supplemental_cohort_excludes_added_cases(self):
        cases=load_cases(ROOT/'evaluation/correction-cases.json')
        baseline=load_baseline(ROOT/'evaluation/correction-baseline/qwen3-4b-preservation.json','qwen3-4b',MODELS['qwen3-4b'],cases)
        rows=[]
        for case in cases:
            rows.append(dict(case=case,text=case['reference'],failed=False,rejected=False,constraints=constraints(case['reference'],case)))
        result=compare_baseline(baseline,rows)
        self.assertEqual(result['before']['completed'],20)
        self.assertEqual(result['after']['completed'],20)
        self.assertEqual(len(rows),28)
        changed=copy.deepcopy(cases);changed[0]['transcript']='changed'
        with self.assertRaises(ValueError):load_baseline(ROOT/'evaluation/correction-baseline/qwen3-4b-preservation.json','qwen3-4b',MODELS['qwen3-4b'],changed)

if __name__=='__main__':unittest.main()
