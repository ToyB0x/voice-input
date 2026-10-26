#!/usr/bin/env python3
"""Compare fresh and reused production Qwen inference; emit numeric evidence only."""
import argparse
import json
import os
from pathlib import Path
import platform
import select
import socket
import subprocess
import time

from correction_models import MODELS
from transcription_models import checksum

PHASES = ('backend_ms', 'model_load_ms', 'context_ms', 'prefill_ms', 'generation_ms', 'validation_ms')
A = dict(transcript='会議は金曜日です', context='会議、金曜日。')
B = dict(transcript='会議は月曜日です', context='会議、月曜日。')


def receive(process, timeout=150):
    deadline = time.monotonic() + timeout
    frame = bytearray()
    while time.monotonic() < deadline:
        ready, _, _ = select.select([process.stdout], [], [], min(0.2, max(0, deadline - time.monotonic())))
        if not ready:
            continue
        data = os.read(process.stdout.fileno(), 4096)
        if not data:
            raise ValueError('Helper closed output before completing a response')
        frame.extend(data)
        if len(frame) > 24576:
            raise ValueError('Helper exceeded output limit')
        if b'\n' in frame:
            if frame[-1:] != b'\n' or frame.count(b'\n') != 1:
                raise ValueError('Unexpected response framing')
            return json.loads(frame)
    raise TimeoutError('Correction deadline exceeded')


def sample(report, wall_ms, reused):
    if report.get('model_reused') is not reused or report.get('backend') != 'cpu' or report.get('offloaded_layers') != 0:
        raise ValueError('Unexpected reuse/backend report')
    p = report.get('performance', {})
    if any(type(p.get(k)) is not int or p[k] < 0 for k in PHASES):
        raise ValueError('Missing timing evidence')
    if p.get('input_tokens', 0) <= 0 or p.get('generated_tokens', 0) <= 0:
        raise ValueError('Expected real inference')
    if reused and (p['backend_ms'] != 0 or p['model_load_ms'] != 0):
        raise ValueError('Reused request initialized the model again')
    # Strict allowlist; do not copy report.text, model metadata or native logs.
    return dict(wall_ms=wall_ms, model_reused=reused, **{k: p[k] for k in PHASES},
                input_tokens=p['input_tokens'], generated_tokens=p['generated_tokens'],
                process_peak_rss_bytes=p.get('peak_rss_bytes'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--model', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    try:
        socket.create_connection(('127.0.0.1', 9), timeout=1)
    except PermissionError:
        pass
    else:
        raise ValueError('Run inside the network-denying sandbox')
    if checksum(args.model) != MODELS['qwen3-1.7b']['sha256']:
        raise ValueError('Unexpected model checksum')
    binary = str(args.binary.resolve())
    if json.loads(subprocess.check_output([binary, '--build-info']))['ci_diagnostics'] is not False:
        raise ValueError('Use the quiet production runtime')
    started = time.monotonic()
    fresh = subprocess.run([binary, str(args.model.resolve()), 'cpu'], input=json.dumps(A).encode(),
                           stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=150, check=True)
    baseline = json.loads(fresh.stdout)
    rows = [sample(baseline, round((time.monotonic() - started) * 1000), False)]
    process = subprocess.Popen([binary, str(args.model.resolve()), 'cpu', '--serve'],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        for index, request in enumerate((A, B, A)):
            started = time.monotonic()
            process.stdin.write(json.dumps(request).encode() + b'\n')
            process.stdin.flush()
            result = receive(process)
            rows.append(sample(result, round((time.monotonic() - started) * 1000), index > 0))
            if index in (0, 2) and result.get('text') != baseline.get('text'):
                raise ValueError('Repeated input differs from fresh inference')
            if index == 1 and ('月曜日' not in result.get('text', '') or '金曜日' in result.get('text', '')):
                raise ValueError('Previous context affected the next request')
            if process.poll() is not None:
                raise ValueError('Resident exited before EOF')
        process.stdin.close()
        if process.wait(timeout=15) != 0:
            raise ValueError('Resident did not exit cleanly after EOF')
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        process.stdout.close()
    evidence = dict(status='passed', backend='cpu', network_denied=True,
                    same_process_requests=3, model_loads_in_resident=1,
                    fresh_output_preserved=True, request_context_isolated=True, eof_exit_code=0,
                    binary_sha256=checksum(args.binary), model_sha256=MODELS['qwen3-1.7b']['sha256'],
                    os=platform.platform(), architecture=platform.machine(), samples=rows,
                    limitation='Fixed synthetic CPU inputs; OS cache and order affect timings. RSS is a process high-water mark, not GPU memory. No speedup threshold or physical Mac performance claim.')
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'reuse.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence))


if __name__ == '__main__':
    main()
