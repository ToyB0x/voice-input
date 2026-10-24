#!/usr/bin/env python3
"""Bounded capability/constraint probe using public synthetic cases, not a quality ranking."""
import argparse, json, platform, socket, subprocess, time
from pathlib import Path
from correction_models import MODELS
from transcription_models import checksum
from evaluate_correction import ROOT, infer, constraints, metal_unavailable

def measured_infer(binary, model, backend, raw, context):
    started = time.monotonic()
    try:
        return infer(binary, model, backend, raw, context)
    except subprocess.TimeoutExpired:
        return dict(wall_ms=(time.monotonic()-started)*1000, peak_rss_bytes=None,
                    rejected=False, failed=True, text=raw, model_report=None,
                    failure=dict(error='runtime', stage='subprocess-timeout'),
                    diagnostic_stderr='Inference exceeded the subprocess time limit.')


def run_probes(binary, model, cases, output, model_name, identity):
    records = []
    report = dict(model=model_name, identity=identity, os=platform.platform(),
                  network_denied=True, cases=records, metal=None, metal_unavailable=False,
                  limitation='Nine fixed synthetic cases only. Not a speech/OCR quality comparison. Rejections retain raw text.')

    def persist():
        # Leave complete prior evidence even if the next case or the job times out.
        temporary = output/'report.json.tmp'
        temporary.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
        temporary.replace(output/'report.json')
        lines = ['| Case | Rejected | Runtime failure | Constraints | Seconds | Peak MiB |', '|---|---|---|---|---:|---:|']
        for r in records:
            rss = f"{r['peak_rss_bytes']/1048576:.1f}" if r['peak_rss_bytes'] is not None else 'unknown'
            lines.append(f"| {r['id']} | {r['rejected']} | {r['failed'] and not r['rejected']} | {r['constraints']} | {r['wall_ms']/1000:.1f} | {rss} |")
        if report['metal'] is None:
            status = 'Metal: not tested yet.'
        elif report['metal_unavailable']:
            status = 'Metal: UNAVAILABLE; inference NOT verified.'
        elif report['metal']['failed']:
            status = 'Metal: FAILED; inference NOT verified.'
        else:
            status = 'Metal: actual inference verified.'
        lines += ['', status, '', report['limitation']]
        temporary = output/'summary.md.tmp'
        temporary.write_text('\n'.join(lines)+'\n')
        temporary.replace(output/'summary.md')

    persist()
    for case in cases:
        result = measured_infer(binary, model, 'cpu', case['transcript'], case['context'])
        records.append(dict(id=case['id'], **result, constraints=constraints(result['text'], case)))
        persist()
        print(case['id'], 'runtime-failed' if result['failed'] and not result['rejected'] else 'completed', flush=True)
    gpu = measured_infer(binary, model, 'metal', cases[0]['transcript'], cases[0]['context'])
    report.update(metal=gpu, metal_unavailable=gpu['failed'] and metal_unavailable(gpu.get('failure', {})))
    persist()
    if any(r['failed'] and not r['rejected'] for r in records) or (gpu['failed'] and not report['metal_unavailable']):
        raise RuntimeError('Model execution failed; see report.json')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--model-name', choices=MODELS, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    try:
        socket.create_connection(('127.0.0.1', 9), timeout=1)
    except PermissionError:
        pass
    else:
        raise ValueError('Expected network sandbox EPERM')
    if checksum(args.model) != MODELS[args.model_name]['sha256']:
        raise ValueError('Model checksum mismatch')
    cases = json.loads((ROOT/'evaluation/correction-cases.json').read_text())['cases']
    run_probes(args.binary, args.model, cases, args.output, args.model_name, MODELS[args.model_name])


if __name__ == '__main__':
    main()
