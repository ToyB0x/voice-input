#!/usr/bin/env python3
"""Compare stronger ASR + 4B combinations, keeping each ASR baseline distinct."""
import argparse
import json
from pathlib import Path
import subprocess
from correction_quality import load_cases
from evaluate_models import ROOT
from evaluate_correction import MODES
from correction_baselines import validate_frozen,render_baseline
from summarize_correction import validate_report, validate_sources

EXPECTED = ['medium', 'large-v3-turbo']

def validate(reports, corpus, cases, revision):
    indexed = {}
    common = None
    for report in reports:
        asr = report.get('asr_model')
        if asr not in EXPECTED or asr in indexed or report['model'] != 'qwen3-4b':
            raise ValueError('Unexpected or duplicate preset combination')
        signature = validate_report(report, corpus, cases, revision)
        comparable = {key:signature[key] for key in ['corpus_sha256','supplemental_sha256',
                      'runtime_source_sha256','cargo_lock_sha256','host_platform']}
        if common is not None and comparable != common:
            raise ValueError('Preset source, corpus or host class differs')
        common = comparable
        indexed[asr] = report
    if set(indexed) != set(EXPECTED):
        raise ValueError('Incomplete preset matrix')
    return indexed

def render(indexed):
    lines = ['| ASR + Qwen3 4B | Context routing | Raw CER | Final CER | Rejected | Mean spoken ms | Peak MiB |',
             '|---|---|---:|---:|---:|---:|---:|']
    for asr in EXPECTED:
        for mode, item in indexed[asr]['summary'].items():
            lines.append(f"| {asr} | {mode} | {item['raw_cer']:.2%} | {item['cer']:.2%} | {item['rejected']} | {item['mean_wall_ms']:.0f} | {item['peak_rss_bytes']/1048576:.1f} |")
    for asr in EXPECTED:
        lines += ['', asr+' preservation comparison:', '', render_baseline(indexed[asr])]
    lines += ['', 'Each ASR model uses its own current-run outputs and raw baseline. All four modes run final correction. '
              'Public read speech with fixed reference-vocabulary hints; this is not a general dictation or live-OCR ranking. '
              'CER normalizes punctuation, so formatting benefits are separate. Rejections keep raw text; quality checks, '
              'full text and diffs are in each artifact. Linux correction times include loading and do not predict Mac GPU performance. '
              'Model presets do not enable screen capture, context routing, permissions or automatic correction adoption.']
    return '\n'.join(lines)+'\n'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports',type=Path);parser.add_argument('output',type=Path)
    args=parser.parse_args()
    reports=[json.loads(p.read_text()) for p in args.reports.rglob('report.json')]
    corpus=json.loads((ROOT/'evaluation/corpus.json').read_text())
    cases=load_cases(ROOT/'evaluation/correction-cases.json')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    indexed=validate(reports,corpus,cases,revision)
    for report in indexed.values():
        validate_sources(report)
        validate_frozen(report,corpus,cases,MODES)
    args.output.mkdir(parents=True,exist_ok=True)
    (args.output/'comparison.json').write_text(json.dumps(indexed,ensure_ascii=False,indent=2)+'\n')
    (args.output/'summary.md').write_text(render(indexed))
    print(render(indexed))

if __name__=='__main__':main()
