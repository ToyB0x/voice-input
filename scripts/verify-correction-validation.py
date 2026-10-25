#!/usr/bin/env python3
"""Check the quiet product validation route against all fixed guard decisions, without a model."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    cases = json.loads((ROOT / 'evaluation/correction-guard-cases.json').read_text())['cases']
    for case in cases:
        payload = json.dumps(dict(transcript=case['original'], candidate=case['candidate']), ensure_ascii=False)
        result = subprocess.run([str(args.binary.resolve()), '--validate'], input=payload, text=True, capture_output=True, timeout=10)
        report = json.loads(result.stdout)
        assert (result.returncode == 0) == case['should_accept'], case['id']
        if result.returncode == 0:
            assert report['text'] == case['candidate'].strip()
        else:
            assert report['error'] == 'rejected' and report['stage'] == 'validation'
    for payload in ['bad-json', 'x' * 24577]:
        result = subprocess.run([str(args.binary.resolve()), '--validate'], input=payload, text=True, capture_output=True, timeout=10)
        assert result.returncode != 0 and 'bad-json' not in result.stdout
    print(f'Product validation: {len(cases)} fixed decisions and bounded invalid input passed; no model loaded.')


if __name__ == '__main__':
    main()
