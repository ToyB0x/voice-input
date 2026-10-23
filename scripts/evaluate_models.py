#!/usr/bin/env python3
"""Offline corpus scoring and process metrics. No downloads or network clients."""
import argparse
import csv
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import socket
import statistics
import subprocess
import sys
import time
import unicodedata

from transcription_models import MODELS, REVISION, checksum

ROOT = Path(__file__).resolve().parent.parent


def normalize(text):
    return ''.join(c for c in unicodedata.normalize('NFKC', text).casefold()
                   if not c.isspace() and not unicodedata.category(c).startswith('P'))


def distance(reference, hypothesis):
    previous = list(range(len(hypothesis) + 1))
    for i, left in enumerate(reference, 1):
        current = [i]
        for j, right in enumerate(hypothesis, 1):
            current.append(min(current[-1] + 1, previous[j] + 1,
                               previous[j - 1] + (left != right)))
        previous = current
    return previous[-1]


def score(reference, hypothesis):
    edits = distance(reference, hypothesis)
    return dict(edits=edits, reference_chars=len(reference),
                cer=edits / len(reference) if reference else None)


def summarize(cases):
    edits = sum(c['normalized']['edits'] for c in cases if c['reference'])
    chars = sum(c['normalized']['reference_chars'] for c in cases if c['reference'])
    spoken = [c for c in cases if c['reference']]
    return dict(edits=edits, reference_chars=chars, cer=edits / chars if chars else None,
                median_inference_ms=statistics.median(c['inference_ms'] for c in spoken) if spoken else None,
                rtf=sum(c['inference_ms'] for c in spoken) / sum(c['audio_seconds'] * 1000 for c in spoken) if spoken else None,
                silence_false_positives=sum(bool(c['hypothesis']) for c in cases if not c['reference']))


def measure(command, output, timeout):
    """wait4 provides per-child peak RSS, rather than a cumulative children maximum."""
    with (output / 'raw.json').open('w') as stdout, (output / 'diagnostics.log').open('w') as stderr:
        process = subprocess.Popen(command, stdout=stdout, stderr=stderr)
        deadline = time.monotonic() + timeout
        try:
            while True:
                pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError(f'benchmark exceeded {timeout} seconds')
                time.sleep(0.1)
        finally:
            if process.returncode is None:
                process.kill()
                process.wait()
        if process.returncode:
            raise RuntimeError(f'benchmark exited {process.returncode}; see diagnostics.log')
    return usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--model-name', choices=MODELS, required=True)
    parser.add_argument('--backend', choices=['cpu', 'metal'], required=True)
    parser.add_argument('--corpus', type=Path, default=ROOT / 'evaluation/corpus.json')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=1200)
    parser.add_argument('--require-network-denied', action='store_true')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if checksum(args.model) != MODELS[args.model_name]['sha256']:
        raise ValueError('Model checksum mismatch')
    corpus = json.loads(args.corpus.read_text())
    expected = {case['id']: case for case in corpus['cases']}
    if len(expected) != len(corpus['cases']):
        raise ValueError('Duplicate corpus IDs')
    for case in corpus['cases']:
        path = (args.corpus.parent / case['path']).resolve()
        if args.corpus.parent.resolve() not in path.parents or checksum(path) != case['sha256']:
            raise ValueError(f'Invalid fixture identity: {case["id"]}')
    if args.require_network_denied:
        try:
            socket.create_connection(('127.0.0.1', 9), timeout=1)
        except PermissionError:
            pass
        else:
            raise RuntimeError('Expected network sandbox EPERM')
    command = [str(args.binary.resolve()), str(args.model.resolve()), str(args.corpus.resolve())]
    if args.backend == 'cpu':
        command.append('--cpu')
    rss = measure(command, args.output, args.timeout)
    raw = json.loads((args.output / 'raw.json').read_text())
    metal = 'whisper_backend_init_gpu: using Metal backend' in (args.output / 'diagnostics.log').read_text()
    if metal != (args.backend == 'metal') or raw['backend'] != args.backend or raw['model_name'] != args.model_name:
        raise ValueError('Requested model/backend was not actually used')
    if [c['id'] for c in raw['cases']] != list(expected):
        raise ValueError('Missing, reordered or unexpected benchmark cases')
    cases = []
    for case in raw['cases']:
        source = expected[case['id']]
        if len(case['observations']) != 2:
            raise ValueError('Expected two retained-model measurements')
        for repeat, observation in enumerate(case['observations'], 1):
            reference, hypothesis = source['reference'], observation['text']
            cases.append(dict(id=case['id'], category=source['category'], repeat=repeat,
                              reference=reference, hypothesis=hypothesis,
                              raw=score(reference, hypothesis), normalized=score(normalize(reference), normalize(hypothesis)),
                              inference_ms=observation['inference_ms'], audio_seconds=case['audio_seconds']))
    summary = summarize(cases)
    if summary['silence_false_positives']:
        raise ValueError('Nonempty recognition on digital silence')
    host = dict(system=platform.system(), release=platform.release(), machine=platform.machine(), logical_cpus=os.cpu_count(), python=platform.python_version())
    if sys.platform == 'darwin':
        for name in ['machdep.cpu.brand_string', 'hw.memsize']:
            host[name] = subprocess.check_output(['/usr/sbin/sysctl', '-n', name], text=True).strip()
    source_revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    source_dirty = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip())
    report = dict(schema_version=1, protocol_version='ja-cer-v1', source_revision=source_revision, source_dirty=source_dirty,
                  cargo_lock_sha256=checksum(ROOT / 'crates/local-transcription/Cargo.lock'), binary_sha256=checksum(args.binary), timestamp=datetime.now(timezone.utc).isoformat(), host=host,
                  model_name=args.model_name, model_sha256=MODELS[args.model_name]['sha256'], model_revision=REVISION,
                  model_bytes=args.model.stat().st_size, backend=args.backend, metal_initialized=metal,
                  corpus_sha256=checksum(args.corpus), network_denied_probe=args.require_network_denied,
                  peak_process_rss_bytes=rss, load_ms=raw['load_ms'], cold_first_inference=raw['cold_first_inference'],
                  summary=summary, by_category={key: summarize([c for c in cases if c['category'] == key]) for key in sorted({c['category'] for c in cases})},
                  cases=cases)
    (args.output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    with (args.output / 'cases.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=['id', 'category', 'repeat', 'reference', 'hypothesis', 'audio_seconds', 'inference_ms', 'raw_edits', 'raw_reference_chars', 'normalized_edits', 'normalized_reference_chars', 'normalized_cer'])
        writer.writeheader()
        for case in cases:
            row = {k: case[k] for k in ['id', 'category', 'repeat', 'reference', 'hypothesis', 'audio_seconds', 'inference_ms']}
            row.update(raw_edits=case['raw']['edits'], raw_reference_chars=case['raw']['reference_chars'],
                       normalized_edits=case['normalized']['edits'], normalized_reference_chars=case['normalized']['reference_chars'], normalized_cer=case['normalized']['cer'])
            writer.writerow(row)
    print(json.dumps({key: report[key] for key in ['model_name', 'backend', 'peak_process_rss_bytes', 'summary']}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
