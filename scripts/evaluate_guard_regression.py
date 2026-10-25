#!/usr/bin/env python3
"""Measure fixed harmful-accept and valid-format-reject cases; no model inference."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
from evaluate_models import ROOT
from transcription_models import checksum

def metrics(cases, results):
    if [r['id'] for r in results] != [c['id'] for c in cases] or any(type(r.get('accepted')) is not bool for r in results):
        raise ValueError('Incomplete or reordered guard observations')
    good=[(c,r) for c,r in zip(cases,results) if c['should_accept']]
    bad=[(c,r) for c,r in zip(cases,results) if not c['should_accept'] and c.get('category')!='conservative-repair']
    policy=[(c,r) for c,r in zip(cases,results) if c.get('category')=='conservative-repair']
    return dict(valid_cases=len(good),accepted_valid=sum(r['accepted'] for c,r in good),
                false_rejections=sum(not r['accepted'] for c,r in good),harmful_cases=len(bad),
                rejected_harmful=sum(not r['accepted'] for c,r in bad),false_accepts=sum(r['accepted'] for c,r in bad),
                policy_cases=len(policy),policy_rejected=sum(not r['accepted'] for c,r in policy))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--require-network-denied',action='store_true')
    args=parser.parse_args()
    if args.require_network_denied:
        try:socket.create_connection(('127.0.0.1',9),timeout=1)
        except PermissionError:pass
        else:raise ValueError('Network denial was not enforced')
    path=ROOT/'evaluation/correction-guard-cases.json'
    cases=json.loads(path.read_text())['cases']
    saved=ROOT/'evaluation/correction-baseline/guard-preservation.json'
    baseline=json.loads(saved.read_text())
    if baseline['cases_sha256']!=checksum(path):raise ValueError('Frozen guard inputs changed')
    result=subprocess.run([str(args.binary.resolve())],input=path.read_text(),text=True,capture_output=True,check=True,timeout=20)
    after=json.loads(result.stdout)
    report=dict(source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                runtime_source_sha256=checksum(ROOT/'crates/local-correction/src/lib.rs'),binary_sha256=checksum(args.binary),
                cases_sha256=checksum(path),baseline_sha256=checksum(saved),network_denied=args.require_network_denied,
                before=metrics(cases,baseline['results']),after=metrics(cases,after),results=after,
                limitation='Fixed public/synthetic expected decisions, not a semantic guarantee or an estimate of real-world error rates. A valid negative self-repair rejected by conservative policy is counted separately, not as a harmful candidate.')
    args.output.mkdir(parents=True,exist_ok=True)
    (args.output/'guard-regression.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    lines=['| Version | Valid accepted | False rejections | Harmful rejected | False accepts | Conservative policy rejections |',
           '|---|---:|---:|---:|---:|---:|']
    for version in ['before','after']:
        m=report[version];lines.append(f"| {version} | {m['accepted_valid']}/{m['valid_cases']} | {m['false_rejections']} | {m['rejected_harmful']}/{m['harmful_cases']} | {m['false_accepts']} | {m['policy_rejected']}/{m['policy_cases']} |")
    lines+=['',report['limitation']]
    (args.output/'summary.md').write_text('\n'.join(lines)+'\n');print('\n'.join(lines))
    if report['after']['false_rejections'] or report['after']['false_accepts'] or report['after']['policy_rejected']!=report['after']['policy_cases']:raise ValueError('Guard regression; inspect fixed decisions')

if __name__=='__main__':main()
