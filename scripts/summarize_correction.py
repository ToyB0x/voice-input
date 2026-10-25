#!/usr/bin/env python3
"""Publish only complete, comparable local correction quality reports."""
import argparse
import json
from pathlib import Path
import subprocess
from correction_models import MODELS
from correction_quality import load_cases, quality_summary, constraints, load_baseline, compare_baseline
from evaluate_correction import MODES, route_summary
from evaluate_models import ROOT, normalize, score
from transcription_models import checksum, MODELS as ASR_MODELS

from correction_baselines import validate_frozen,render_baseline

EXPECTED = ['qwen3-1.7b', 'qwen3-4b', 'qwen3-8b']


def validate_report(report, corpus, cases, revision):
    name = report['model']
    if name not in MODELS:
        raise ValueError('Unexpected correction model')
    case_ids = [case['id'] for case in corpus['cases']]
    supplemental_ids = [case['id'] for case in cases]
    if (report.get('protocol_version') != 'correction-quality-v2' or not report.get('complete')
        or report.get('source_revision') != revision or report.get('source_dirty')
        or not report.get('network_denied') or report.get('backend') != 'cpu'
        or report.get('model_identity') != MODELS[name]
        or not report.get('metal_skipped') or report.get('metal_smoke') is not None):
        raise ValueError('Unverified or incomplete correction report')
    signature = {key: report[key] for key in ['asr_input_sha256', 'asr_binary_sha256',
                 'corpus_sha256', 'supplemental_sha256', 'runtime_source_sha256', 'cargo_lock_sha256']}
    signature['host_platform'] = {key: report['host'][key] for key in ['system', 'machine', 'logical_cpus']}
    signature['asr_observations'] = [(row['id'], row['mode'], row['raw_asr']) for row in report['records']]
    if report['host']['system'] != 'Linux':
        raise ValueError('Expected Linux CPU comparison')
    if (report['expected_records'] != len(case_ids)*len(MODES)
        or report['expected_supplemental'] != len(supplemental_ids)
        or [(row['id'], row['mode']) for row in report['records']] != [(case, mode) for case in case_ids for mode in MODES]
        or [row['case']['id'] for row in report['supplemental']] != supplemental_ids):
        raise ValueError('Missing, duplicate or reordered correction cases')
    references = {case['id']: case for case in corpus['cases']}
    raw_inputs = {}
    for row in report['records']:
        case = references[row['id']]
        source = (row['id'], MODES[row['mode']][0])
        if source in raw_inputs and raw_inputs[source] != row['raw_asr']:
            raise ValueError('ASR input changed across correction-only routing')
        raw_inputs[source] = row['raw_asr']
        expected_context = case['hint'] if MODES[row['mode']][1] else ''
        if (row['reference'] != case['reference'] or row['context'] != expected_context
            or row['raw_score'] != score(normalize(case['reference']), normalize(row['raw_asr']))
            or row['final_score'] != score(normalize(case['reference']), normalize(row['text']))
            or (row['failed'] and row['text'] != row['raw_asr'])
            or (row['rejected'] and not row['failed'])):
            raise ValueError('Correction scores or routing do not match source observations')
    for row, case in zip(report['supplemental'], cases):
        if (row['case'] != case or row['constraints'] != constraints(row['text'], case)
            or (row['failed'] and row['text'] != case['transcript'])
            or (row['rejected'] and not row['failed'])):
            raise ValueError('Quality checks or fallbacks do not match declared cases')
    if report['summary'] != route_summary(report['records']) or report['quality'] != quality_summary(report['supplemental']):
        raise ValueError('Correction metrics do not match observations')
    if any(row['failed'] and not row['rejected'] for row in report['records']+report['supplemental']):
        raise ValueError('Correction runtime failure; inspect per-model evidence')
    if any(row['silence_false_positives'] for row in report['summary'].values()) or report['quality']['silence_false_positives']:
        raise ValueError('Silence invariant failed')
    asr = report.get('asr_model', 'small')
    if asr not in ASR_MODELS or report.get('asr_model_sha256', ASR_MODELS['small']['sha256']) != ASR_MODELS[asr]['sha256']:
        raise ValueError('Unexpected ASR model identity')
    signature['asr_model'] = asr
    return signature


def validate_sources(report):
    if (report['corpus_sha256'] != checksum(ROOT/'evaluation/corpus.json')
        or report['supplemental_sha256'] != checksum(ROOT/'evaluation/correction-cases.json')
        or report['runtime_source_sha256'] != checksum(ROOT/'crates/local-correction/src/lib.rs')
        or report['cargo_lock_sha256'] != checksum(ROOT/'crates/local-correction/Cargo.lock')):
        raise ValueError('Reports are stale relative to the checked-out source')


def validate_reports(reports, corpus, cases, revision):
    indexed = {}
    common = None
    for report in reports:
        name = report['model']
        if name not in EXPECTED or name in indexed:
            raise ValueError('Unexpected or duplicate correction model')
        signature = validate_report(report, corpus, cases, revision)
        if common is not None and signature != common:
            raise ValueError('Correction inputs, protocol or source are not comparable')
        common = signature
        indexed[name] = report
    if set(indexed) != set(EXPECTED):
        raise ValueError('Incomplete correction model matrix')
    return indexed


def render(indexed):
    lines = ['| Model | Context routing | Raw CER | Final CER | Rejected | Mean ms | Peak MiB |',
             '|---|---|---:|---:|---:|---:|---:|']
    for name in EXPECTED:
        for mode, item in indexed[name]['summary'].items():
            lines.append(f"| {name} | {mode} | {item['raw_cer']:.2%} | {item['cer']:.2%} | {item['rejected']} | {item['mean_wall_ms']:.0f} | {item['peak_rss_bytes']/1048576:.1f} |")
    lines += ['', 'Fixed synthetic-case quality (only generated nonempty candidates; fallbacks excluded):', '',
              '| Model | Generated | Facts pass | Format pass | Both pass | Rejected | Runtime failures |',
              '|---|---:|---:|---:|---:|---:|---:|']
    for name in EXPECTED:
        q = indexed[name]['quality']
        lines.append(f"| {name} | {q['generated']} | {q['facts_passed']} | {q['format_passed']} | {q['both_passed']} | {q['rejected']} | {q['runtime_failures']} |")
    baseline = indexed['qwen3-8b'].get('baseline')
    if baseline:
        lines += ['', '8B frozen baseline, same original inputs and current checks:', '',
                  '| Version | Generated | Facts pass | Format pass | Both pass | Rejected |',
                  '|---|---:|---:|---:|---:|---:|']
        for version in ['before', 'after']:
            q = baseline[version]
            lines.append(f"| {version} | {q['generated']} | {q['facts_passed']} | {q['format_passed']} | {q['both_passed']} | {q['rejected']} |")
        lines += ['', baseline['limitation']]
    lines += ['', '4B preservation comparison:', '', render_baseline(indexed['qwen3-4b'])]
    lines += ['', 'CER uses public read speech with fixed reference-vocabulary contexts. Fact/format checks apply only '
              'to declared synthetic cases and do not establish general semantic equivalence. Rejection retains raw text '
              'and is not a generated success. No GPU inference was tested. Timings include model loading on separate '
              'Linux VMs and do not predict physical Mac performance. Raw ASR, previews and diffs are in the model artifacts.']
    return '\n'.join(lines)+'\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    corpus = json.loads((ROOT/'evaluation/corpus.json').read_text())
    cases = load_cases(ROOT/'evaluation/correction-cases.json')
    reports = [json.loads(path.read_text()) for path in args.reports.rglob('report.json')]
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    indexed = validate_reports(reports, corpus, cases, revision)
    for report in indexed.values():
        validate_sources(report)
        validate_frozen(report,corpus,cases,MODES)
    baseline_path = ROOT/'evaluation/correction-baseline/qwen3-8b.json'
    baseline = load_baseline(baseline_path, 'qwen3-8b', MODELS['qwen3-8b'], cases)
    if (indexed['qwen3-8b']['baseline_sha256'] != checksum(baseline_path)
        or indexed['qwen3-8b']['baseline'] != compare_baseline(baseline, indexed['qwen3-8b']['supplemental'])):
        raise ValueError('Missing or invalid frozen baseline comparison')
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output/'comparison.json').write_text(json.dumps(indexed, ensure_ascii=False, indent=2)+'\n')
    (args.output/'summary.md').write_text(render(indexed))
    baseline = indexed['qwen3-8b'].get('baseline')
    if baseline:
        (args.output/'baseline.diff').write_text('\n'.join(pair['diff'] for pair in baseline['pairs']))
    print(render(indexed))


if __name__ == '__main__':
    main()
