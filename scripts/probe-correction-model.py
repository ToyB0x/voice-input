#!/usr/bin/env python3
"""Bounded capability/constraint probe using public synthetic cases, not a quality ranking."""
import argparse, json, platform, socket
from pathlib import Path
from correction_models import MODELS
from transcription_models import checksum
from evaluate_correction import ROOT, infer, constraints, metal_unavailable

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
records = []
for case in cases:
    result = infer(args.binary, args.model, 'cpu', case['transcript'], case['context'])
    records.append(dict(id=case['id'], **result, constraints=constraints(result['text'], case)))
    print(case['id'], 'runtime-failed' if result['failed'] and not result['rejected'] else 'completed', flush=True)
gpu = infer(args.binary, args.model, 'metal', cases[0]['transcript'], cases[0]['context'])
unavailable = gpu['failed'] and metal_unavailable(gpu.get('failure', {}))
report = dict(model=args.model_name, identity=MODELS[args.model_name], os=platform.platform(),
              network_denied=True, cases=records, metal=gpu, metal_unavailable=unavailable,
              limitation='Nine fixed synthetic cases only. Not a speech/OCR quality comparison. Rejections retain raw text.')
(args.output/'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
lines = ['| Case | Rejected | Runtime failure | Constraints | Seconds | Peak MiB |', '|---|---|---|---|---:|---:|']
for r in records:
    lines.append(f"| {r['id']} | {r['rejected']} | {r['failed'] and not r['rejected']} | {r['constraints']} | {r['wall_ms']/1000:.1f} | {(r['peak_rss_bytes'] or 0)/1048576:.1f} |")
lines += ['', 'Metal: UNAVAILABLE; inference NOT verified.' if unavailable else 'Metal: actual inference tested.', '', report['limitation']]
(args.output/'summary.md').write_text('\n'.join(lines)+'\n')
if any(r['failed'] and not r['rejected'] for r in records) or (gpu['failed'] and not unavailable):
    raise RuntimeError('Model execution failed; see report.json')
