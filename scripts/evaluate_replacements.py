#!/usr/bin/env python3
"""Real local-model evaluation on fixed synthetic text. Never accepts user recordings."""
import argparse
import hashlib
import json
import platform
import socket
import subprocess
from pathlib import Path
from correction_models import MODELS
from transcription_models import checksum
from evaluate_correction import run_inference

ROOT = Path(__file__).resolve().parents[1]

def evaluate(binary, model, payload, backend):
    try:
        result = run_inference([str(binary.resolve()), str(model.resolve()), backend, '--compare'],
                               json.dumps(payload, ensure_ascii=False), timeout=1900)
        parsed = json.loads(result.stdout)
        if result.returncode:
            return dict(status='runtime-error', error=parsed)
        if parsed.get('backend') != backend or (backend == 'cpu' and parsed.get('offloaded_layers') != 0):
            raise ValueError('backend mismatch')
        if backend == 'metal' and parsed.get('offloaded_layers', 0) <= 0:
            raise ValueError('Metal was not used')
        return dict(status='accepted' if parsed['accepted'] else 'rejected', **parsed)
    except (subprocess.TimeoutExpired, ValueError, KeyError) as error:
        return dict(status='runtime-error', error=type(error).__name__)

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--model', type=Path, required=True)
    p.add_argument('--model-name', choices=MODELS, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--backend', choices=['cpu','metal'], default='cpu')
    p.add_argument('--repeats', type=int, choices=[1,2,3], default=2)
    p.add_argument('--require-network-denied', action='store_true')
    p.add_argument('--baseline-rewrite', action='store_true')
    p.add_argument('--observe-only', action='store_true', help='Report accuracy for a challenger; runtime failures still fail')
    p.add_argument('--suite', choices=['development','confirmation'], default='development')
    args = p.parse_args()
    if args.require_network_denied:
        try: socket.create_connection(('127.0.0.1',9),timeout=1)
        except PermissionError: pass
        else: raise ValueError('Network denial not enforced')
    if checksum(args.model) != MODELS[args.model_name]['sha256']: raise ValueError('Wrong model')
    fixture = ROOT/'evaluation'/('replacement-cases.json' if args.suite=='development' else 'replacement-confirmation-cases.json')
    if args.baseline_rewrite and args.suite!='development':raise ValueError('Rewrite baseline is defined only for development cases')
    corpus = json.loads(fixture.read_text())
    args.output.mkdir(parents=True,exist_ok=True)
    report = dict(schema_version=1, complete=False, model=args.model_name,
        model_sha256=checksum(args.model), binary_sha256=checksum(args.binary), fixture_sha256=checksum(fixture),
        source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        backend=args.backend, platform=platform.platform(), network_denied=args.require_network_denied,
        mode='rewrite' if args.baseline_rewrite else 'replacements', repeats=args.repeats, rows=[],
        limitation='Fixed synthetic text, not independent speech or captured images. Reading dictionary is caller-supplied; same-reading correctness is not inferred by the validator. Wrong context can still mislead.')
    selected = corpus['cases'] if not args.baseline_rewrite else [c for c in corpus['cases'] if c['id'] in ['recipient','recipient-context','candy-original','candy-ocr-raw','contrast','injection']]
    def save():
        (args.output/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    save()
    for repeat in range(args.repeats):
        for case in selected if repeat % 2 == 0 else reversed(selected):
            payload = dict(transcript=case['transcript'],context=case['context'])
            if not args.baseline_rewrite:
                payload.update(mode='replacements',dictionary=corpus['dictionary'])
            outcome = evaluate(args.binary,args.model,payload,args.backend)
            passed = outcome['status']=='accepted' and outcome.get('candidate')==case['expected']
            report['rows'].append(dict(id=case['id'],repeat=repeat,group=case['group'],gate=case['gate'],
                request=payload,expected=case['expected'],outcome=outcome,passed=passed,
                final_text=outcome.get('candidate') if outcome['status']=='accepted' else case['transcript']))
            save()
            print(f"{repeat} {case['id']}: {outcome['status']} exact={passed}",flush=True)
    rows = report['rows']
    report['complete'] = len(rows)==len(selected)*args.repeats
    report['groups'] = {group:dict(passed=sum(r['passed'] for r in rows if r['group']==group),
        total=sum(r['group']==group for r in rows)) for group in sorted({r['group'] for r in rows})}
    report['unstable_cases'] = [c['id'] for c in selected if len({json.dumps(r['outcome'].get('candidate'),ensure_ascii=False) + r['outcome']['status'] for r in rows if r['id']==c['id']})>1]
    # Strict on the small development suite. This is not a population accuracy claim.
    report['quality_pass'] = report['complete'] and all(r['passed'] for r in rows if r['gate']) and not report['unstable_cases']
    save()
    lines = [f"# {args.model_name} / {report['mode']}", '',f"Complete: {report['complete']}; quality gate: {report['quality_pass']}",'',
        '| Group | Exact accepted outputs | Total |','|---|---:|---:|']
    for group, values in report['groups'].items(): lines.append(f"| {group} | {values['passed']} | {values['total']} |")
    lines += ['', 'Fallbacks are excluded from successful model outputs.',report['limitation'],'',
        'Failed cases: '+', '.join(sorted({r['id'] for r in rows if not r['passed']})),
        'Unstable cases: '+', '.join(report['unstable_cases'])]
    (args.output/'summary.md').write_text('\n'.join(lines)+'\n')
    if any(r['outcome']['status']=='runtime-error' for r in rows): raise SystemExit(1)
    if not args.baseline_rewrite and not args.observe_only and not report['quality_pass']: raise SystemExit(1)
if __name__=='__main__': main()
