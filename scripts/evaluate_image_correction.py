#!/usr/bin/env python3
"""Replay public ASR text through correction using actual, hashed VLM observations."""
import argparse
import json
import socket
import subprocess
from pathlib import Path
from correction_models import MODELS
from transcription_models import checksum
from evaluate_replacements import evaluate
from observation_lexicon import load_lexicon, annotate
ROOT=Path(__file__).resolve().parents[1]
SCENES={'storm':('雨ですね。','雨ですね。'),'candy':('雨ですね。','飴ですね。'),
        'rainfall':('雨が降っています。','雨が降っています。'),'candy-shower':('雨が降っています。','飴が降っています。')}
DICTIONARY=[dict(reading='あめ',forms=['雨','飴','あめ'])]

def observation_context(row):
    observation=row['observation']
    if set(observation)!={'objects','actions','visible_text','uncertain'}:raise ValueError('Unexpected observation schema')
    for values in observation.values():
        if not isinstance(values,list) or len(values)>6 or any(not isinstance(v,str) or len(v)>40 for v in values):
            raise ValueError('Observation exceeds bounds')
    # Uncertain guesses never become affirmative evidence. No reference answers,
    # scene ID, or raw VLM prose is added to the correction model's context.
    return '\n'.join(label+'：'+value+'。' for key,label in [('objects','物体'),('actions','動作'),('visible_text','画像内の文字')]
                     for value in observation[key])

def load_observations(path):
    if path.stat().st_size>2_000_000:raise ValueError('Observation report exceeds bounds')
    report=json.loads(path.read_text())
    if not report.get('complete') or not report.get('runtime_pass'):raise ValueError('Incomplete VLM inference')
    rows=report['rows']
    if len(rows)!=8 or {(r['scene'],r['repeat']) for r in rows}!={(s,i) for s in SCENES for i in [0,1]}:
        raise ValueError('Missing or duplicate observation')
    for row in rows:
        if row['status']!='completed' or row['image_sha256']!=checksum(ROOT/'evaluation/context-evidence/assets'/f"{row['scene']}.png"):
            raise ValueError('Unverified image observation')
        observation_context(row)
    return report

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--observations',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--model',type=Path,required=True)
    p.add_argument('--model-name',choices=['qwen3-8b','qwen3-14b'],default='qwen3-8b')
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--require-network-denied',action='store_true')
    p.add_argument('--lexicon',type=Path,help='Explicit caller-supplied synonyms; omitted for the literal baseline')
    args=p.parse_args()
    if args.require_network_denied:
        try:socket.create_connection(('127.0.0.1',9),timeout=1)
        except PermissionError:pass
        else:raise ValueError('Network denial not enforced')
    if checksum(args.model)!=MODELS[args.model_name]['sha256']:raise ValueError('Wrong correction model')
    upstream=load_observations(args.observations)
    aliases=load_lexicon(args.lexicon) if args.lexicon else {}
    args.output.mkdir(parents=True,exist_ok=True)
    report=dict(complete=False,source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        binary_sha256=checksum(args.binary),model=args.model_name,model_sha256=checksum(args.model),
        observation_report_sha256=checksum(args.observations),upstream=upstream,network_denied=args.require_network_denied,rows=[],
        lexicon=aliases,lexicon_sha256=checksum(args.lexicon) if args.lexicon else None,
        limitation='Public synthetic images and replayed ASR text, not a fresh audio-to-text run or independent speech. Image observation is answer-blind; correction receives a caller-supplied homophone dictionary. Mismatched observations can mislead.')
    cases=[]
    for raw in sorted({raw for raw,_ in SCENES.values()}):cases.append(dict(group='no-context',raw=raw,expected=raw,context='',gate=True))
    for row in upstream['rows']:
        raw,expected=SCENES[row['scene']]
        cases.append(dict(group='image-context',scene=row['scene'],repeat=row['repeat'],raw=raw,expected=expected,context=observation_context(row),gate=True))
        if row['scene'] in ['rainfall','candy-shower']:
            other='candy-shower' if row['scene']=='rainfall' else 'rainfall'
            mismatched=next(r for r in upstream['rows'] if r['scene']==other and r['repeat']==row['repeat'])
            cases.append(dict(group='mismatched-context',scene=row['scene'],repeat=row['repeat'],raw=raw,expected=expected,context=observation_context(mismatched),gate=False))
    def save():(args.output/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    save()
    for case in cases:
        case['raw_context']=case['context']
        case['context']=annotate(case['context'],aliases)
        outcome=evaluate(args.binary,args.model,dict(mode='replacements',transcript=case['raw'],context=case['context'],dictionary=DICTIONARY),'cpu')
        passed=outcome['status']=='accepted' and outcome.get('candidate')==case['expected']
        report['rows'].append(dict(case,outcome=outcome,passed=passed));save()
        print(case['group'],case.get('scene','baseline'),passed,flush=True)
    report['complete']=len(report['rows'])==len(cases)
    report['runtime_pass']=all(r['outcome']['status']!='runtime-error' for r in report['rows'])
    report['quality_pass']=report['complete'] and report['runtime_pass'] and all(r['passed'] for r in report['rows'] if r['gate'])
    save()
    lines=['# Image observation → constrained correction','',report['limitation'],'',f"Quality gate: {report['quality_pass']}",'', '| Condition | Scene | Repeat | Output | Exact accepted |','|---|---|---:|---|---|']
    for r in report['rows']:lines.append(f"| {r['group']} | {r.get('scene','baseline')} | {r.get('repeat','')} | {r['outcome'].get('candidate','')} | {r['passed']} |")
    (args.output/'summary.md').write_text('\n'.join(lines)+'\n')
    if not report['quality_pass']:raise SystemExit(1)
if __name__=='__main__':main()
