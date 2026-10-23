#!/usr/bin/env python3
"""Require a complete, comparable model/backend set before producing rankings."""
import argparse
import json
from pathlib import Path
from transcription_models import MODELS


def validate(reports, models, require_offline=False):
    expected = {(model, backend) for model in models for backend in ['cpu', 'metal']}
    identities = [(r['model_name'], r['backend']) for r in reports]
    if len(identities) != len(set(identities)) or set(identities) != expected:
        raise ValueError('Incomplete or duplicate model/backend reports')
    if len({r['corpus_sha256'] for r in reports}) != 1:
        raise ValueError('Corpus identities differ; results cannot be compared')
    for report in reports:
        if report['model_sha256'] != MODELS[report['model_name']]['sha256']:
            raise ValueError('Unexpected model identity')
        if require_offline and not report['network_denied_probe']:
            raise ValueError('Offline enforcement was not verified')
        if report['summary']['cer'] is None or report['summary']['silence_false_positives']:
            raise ValueError('Invalid quality result')
    return sorted(reports, key=lambda r: (r['model_bytes'], r['backend']))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--models', default='["base", "small"]')
    parser.add_argument('--require-network-denied', action='store_true')
    args = parser.parse_args()
    reports = validate([json.loads(path.read_text()) for path in args.reports.glob('**/report.json')],
                       json.loads(args.models), args.require_network_denied)
    lines = ['# Japanese ASR comparison', '',
             'Fixed 7-utterance FLEURS read-speech subset + digital silence; two measured repetitions with a retained model.',
             'CER: NFKC/casefold, punctuation and whitespace removed. Lower is better; spelling variants remain errors.', '',
             '| Model | Backend | CER | RTF | Median inference (s) | Peak process RSS (MiB) | Load (s) | Cold first inference (s) |',
             '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    for report in reports:
        s = report['summary']
        lines.append(f"| {report['model_name']} | {report['backend']} | {s['cer']:.2%} | {s['rtf']:.3f} | {s['median_inference_ms']/1000:.3f} | {report['peak_process_rss_bytes']/1048576:.1f} | {report['load_ms']/1000:.3f} | {report['cold_first_inference']['inference_ms']/1000:.3f} |")
    cpu = [r for r in reports if r['backend'] == 'cpu']
    speed = min(cpu, key=lambda r: r['summary']['rtf'])['model_name']
    accuracy = min(cpu, key=lambda r: (r['summary']['cer'], r['summary']['rtf']))['model_name']
    lines += ['', f'Provisional CPU ranking on this fixed subset: speed = **{speed}**, accuracy = **{accuracy}**.',
              '', 'These are small-sample results on GitHub macOS runners, not a general model recommendation or a physical-Mac GPU comparison.',
              'Peak RSS is per-process ru_maxrss and does not measure all GPU/driver allocations. Cold first inference uses the first corpus case, outside measured repetitions.',
              'Per-case references, hypotheses, raw/normalized errors, timings and host metadata are included in the JSON/CSV artifacts.']
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'summary.md').write_text('\n'.join(lines) + '\n')
    (args.output / 'comparison.json').write_text(json.dumps(reports, ensure_ascii=False, indent=2) + '\n')
    (args.output / 'recommendations.json').write_text(json.dumps(dict(scope='fixed read-speech subset; CPU ranking only',
          speed_model=speed, accuracy_model=accuracy, corpus_sha256=reports[0]['corpus_sha256']), indent=2) + '\n')
    print('\n'.join(lines))


if __name__ == '__main__':
    main()
