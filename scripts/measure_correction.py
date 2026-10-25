#!/usr/bin/env python3
"""Measure fresh correction processes on fixed synthetic input; save numeric metadata only."""
import argparse
import json
from pathlib import Path
import platform
import socket
import subprocess

from correction_models import MODELS
from evaluate_correction import infer
from evaluate_models import ROOT
from transcription_models import checksum

PHASES = ('backend_ms', 'model_load_ms', 'context_ms', 'prefill_ms',
          'generation_ms', 'validation_ms')
COUNTS = ('input_tokens', 'generated_tokens')
TRANSCRIPT = '会議は金曜日です'
CONTEXT = '会議、金曜日。'
LIMITATION = ('Fresh process and model load on every invocation. First observed run is not a '
              'guaranteed cold cache: model checksum verification reads the file first. '
              'Later runs may benefit from OS caches; weights and KV cache are not reused. '
              'Process peak RSS is not total GPU memory. VM results do not predict physical Mac performance.')


def numeric_sample(item, index):
    if item['failed']:
        raise ValueError('Performance sample failed or was rejected; no timing comparison published')
    report = item['model_report']
    metrics = report.get('performance')
    if not isinstance(metrics, dict):
        raise ValueError('Runtime does not provide performance metrics')
    values = {key: metrics.get(key) for key in (*PHASES, *COUNTS, 'peak_rss_bytes')}
    if any(type(value) is not int or value < 0 for value in values.values()):
        raise ValueError('Missing or invalid numeric performance metric')
    elapsed = report.get('elapsed_ms')
    if type(elapsed) is not int or elapsed < sum(values[key] for key in PHASES):
        raise ValueError('Invalid phase timing boundaries')
    if not all(values[key] > 0 for key in (*COUNTS, 'peak_rss_bytes')):
        raise ValueError('Expected real model inference and process memory measurement')
    # Explicit allowlist: never serialize the model report, text, prompt or stderr.
    return dict(invocation=index, process_reused=False, elapsed_ms=elapsed,
                wall_ms=item['wall_ms'], external_peak_rss_bytes=item['peak_rss_bytes'], **values)


def write_results(output, report):
    output.mkdir(parents=True, exist_ok=True)
    (output / 'performance.json').write_text(json.dumps(report, indent=2) + '\n')
    lines = ['| Run (fresh process) | Wall ms | Backend ms | Load ms | Context ms | Prefill ms | Generate ms | Validate ms | Peak MiB |',
             '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
    for row in report['samples']:
        label = 'First observed' if row['invocation'] == 1 else f"Subsequent {row['invocation'] - 1}"
        numbers = [row['wall_ms'], *(row[key] for key in PHASES), row['peak_rss_bytes'] / 1048576]
        lines.append('| ' + label + ' | ' + ' | '.join(f'{value:.1f}' for value in numbers) + ' |')
    lines += ['', f"Complete: {report.get('complete', False)}.", '', f"Model: {report['model']}; backend: {report['backend']}; network denied: {report['network_denied']}.",
              '', LIMITATION]
    (output / 'summary.md').write_text('\n'.join(lines) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--model-name', choices=MODELS, default='qwen3-1.7b')
    parser.add_argument('--backend', choices=('cpu', 'metal'), default='cpu')
    parser.add_argument('--runs', type=int, choices=range(2, 6), default=3)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    # Refuse unrestricted inference. Downloads belong to the explicit setup step.
    try:
        socket.create_connection(('127.0.0.1', 9), timeout=1)
    except PermissionError:
        pass
    else:
        raise ValueError('Expected network sandbox EPERM')
    if checksum(args.model) != MODELS[args.model_name]['sha256']:
        raise ValueError('Model checksum mismatch')
    build_info = subprocess.check_output([str(args.binary.resolve()), '--build-info'], text=True)
    if json.loads(build_info).get('ci_diagnostics') is not False:
        raise ValueError('Use the quiet production runtime')
    report = dict(protocol_version='correction-performance-v1', complete=False,
                  model=args.model_name, model_sha256=MODELS[args.model_name]['sha256'],
                  binary_sha256=checksum(args.binary), backend=args.backend,
                  source_revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                  source_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=ROOT)),
                  os=platform.platform(), architecture=platform.machine(), network_denied=True,
                  input_fixture='fixed-meeting-v1', requested_runs=args.runs,
                  limitation=LIMITATION, samples=[])
    write_results(args.output, report)
    for index in range(1, args.runs + 1):
        item = infer(args.binary, args.model, args.backend, TRANSCRIPT, CONTEXT)
        report['samples'].append(numeric_sample(item, index))
        write_results(args.output, report)
    report['complete'] = True
    write_results(args.output, report)


if __name__ == '__main__':
    main()
