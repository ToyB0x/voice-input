"""Frozen 4B comparisons: identical requests only, no retrospective timing claims."""
from pathlib import Path
from correction_quality import load_baseline, compare_baseline, diff_text
from correction_models import MODELS
from transcription_models import MODELS as ASR_MODELS, checksum
from evaluate_models import ROOT, normalize, score

BASELINE = ROOT/'evaluation/correction-baseline/qwen3-4b-preservation.json'

def load_routes(baseline, asr_model, corpus, modes):
    if not baseline.get('routes'):
        return None
    if baseline.get('corpus_sha256') != checksum(ROOT/'evaluation/corpus.json'):
        raise ValueError('Frozen route corpus changed')
    group=baseline['routes'][asr_model]
    if group['model_sha256'] != ASR_MODELS[asr_model]['sha256']:
        raise ValueError('Frozen route ASR identity mismatch')
    if [(r['id'],r['mode']) for r in group['records']] != [(c['id'],m) for c in corpus['cases'] for m in modes]:
        raise ValueError('Incomplete frozen route inputs')
    by_id={case['id']:case for case in corpus['cases']}
    for row in group['records']:
        case=by_id[row['id']]
        expected_context=case['hint'] if modes[row['mode']][1] else ''
        if row['reference']!=case['reference'] or row['context']!=expected_context:
            raise ValueError('Frozen reference or context differs from corpus')
        if any(not isinstance(row.get(k),str) for k in ['id','mode','raw_asr','reference','context']):
            raise ValueError('Invalid frozen request')
        result=row['result']
        if (not isinstance(result['text'],str) or any(type(result[k]) is not bool for k in ['failed','rejected'])
            or (result['rejected'] and not result['failed']) or (result['failed'] and result['text']!=row['raw_asr'])):
            raise ValueError('Invalid frozen route outcome')
    return dict(group,asr_model=asr_model)

def compare_routes(group, records):
    indexed={(r['id'],r['mode']):r for r in group['records']}
    if len(indexed)!=len(group['records']) or len(records)!=len(indexed) or {(r['id'],r['mode']) for r in records}!=set(indexed):
        raise ValueError('Missing, duplicate or unexpected current route')
    pairs=[];skipped=[]
    for row in records:
        old=indexed[(row['id'],row['mode'])]
        if any(old[k]!=row[k] for k in ['raw_asr','reference','context']):
            skipped.append(dict(id=row['id'],mode=row['mode'],reason='request-changed'))
            continue
        pairs.append(dict(id=row['id'],mode=row['mode'],reference=row['reference'],raw_asr=row['raw_asr'],
                          before=old['result'],after={k:row[k] for k in ['text','failed','rejected']},
                          diff=diff_text(old['result']['text'],row['text'],group['asr_model']+'/'+row['id']+'/'+row['mode'])))
    if not pairs:raise ValueError('No identical frozen route requests to compare')
    summary={}
    def metric(rows,version):
        spoken=[r for r in rows if r['reference']]
        scores=[score(normalize(r['reference']),normalize(r[version]['text'])) for r in spoken]
        chars=sum(s['reference_chars'] for s in scores);edits=sum(s['edits'] for s in scores)
        return dict(cer=edits/chars if chars else None,edits=edits,reference_chars=chars,
                    generated=sum(not r[version]['failed'] and not r[version]['rejected'] for r in spoken),
                    rejected=sum(r[version]['rejected'] for r in rows))
    for mode in dict.fromkeys(row['mode'] for row in records):
        rows=[r for r in pairs if r['mode']==mode]
        if not any(r['reference'] for r in rows):raise ValueError('No identical spoken requests for a route')
        summary[mode]=dict(matched=len(rows),before=metric(rows,'before'),after=metric(rows,'after'))
    return dict(asr_model=group['asr_model'],source_revision=group['source_revision'],run_url=group['run_url'],
                matched=len(pairs),expected=len(indexed),skipped_inputs=skipped,summary=summary,pairs=pairs,
                limitation='Only byte-identical raw ASR/context/reference requests are paired. Changed requests are excluded and counted. '
                'Current CER normalization is applied to both versions. Rejected candidates retain raw text and are reported separately. '
                'Frozen prior run, not a randomized trial or timing comparison.')

def validate_frozen(report, corpus, cases, modes):
    if report['model']!='qwen3-4b':return
    baseline=load_baseline(BASELINE,'qwen3-4b',MODELS['qwen3-4b'],cases)
    routes=load_routes(baseline,report['asr_model'],corpus,modes)
    if (report.get('baseline_sha256')!=checksum(BASELINE)
        or report.get('baseline')!=compare_baseline(baseline,report['supplemental'])
        or report.get('route_baseline')!=compare_routes(routes,report['records'])):
        raise ValueError('Missing or invalid 4B preservation comparison')

def render_baseline(report):
    comparison=report.get('baseline')
    if not comparison:return ''
    lines=['Frozen supplemental inputs, re-scored with the current same declared checks:',
           '', '| Version | Generated | Facts pass | Format pass | Both pass | Rejected |',
           '|---|---:|---:|---:|---:|---:|']
    for version in ['before','after']:
        q=comparison[version]
        lines.append(f"| {version} | {q['generated']} | {q['facts_passed']} | {q['format_passed']} | {q['both_passed']} | {q['rejected']} |")
    lines+=['','Final text checks including raw fallbacks (not generated successes and not a semantic guarantee):','',
            '| Version | Nonempty cases | Facts pass | Missing required cases | Forbidden term cases |',
            '|---|---:|---:|---:|---:|']
    for version in ['before','after']:
        q=comparison[version]
        lines.append(f"| {version} | {q['final_checked']} | {q['final_facts_passed']} | {q['missing_required_cases']} | {q['forbidden_cases']} |")
    routes=report.get('route_baseline')
    if routes:
        lines+=['',f"Frozen route comparison: {routes['matched']}/{routes['expected']} identical requests; {len(routes['skipped_inputs'])} changed inputs excluded.",'',
                '| Context routing | Matched | Before CER | After CER | Before rejected | After rejected |',
                '|---|---:|---:|---:|---:|---:|']
        def percent(value):return 'unmatched' if value is None else f'{value:.2%}'
        for mode,row in routes['summary'].items():
            b,a=row['before'],row['after']
            lines.append(f"| {mode} | {row['matched']} | {percent(b['cer'])} | {percent(a['cer'])} | {b['rejected']} | {a['rejected']} |")
        lines+=['',routes['limitation']]
    else:lines+=['',comparison['limitation']]
    return '\n'.join(lines)+'\n'
