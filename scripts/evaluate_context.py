#!/usr/bin/env python3
"""Compare opt-in local hints and unrelated-hint contamination on public fixtures."""
import argparse
import json
from pathlib import Path
import subprocess
from evaluate_models import ROOT, normalize, score, summarize
from transcription_models import MODELS, checksum


def compare(corpus, reports):
    cases = []
    for case in corpus['cases']:
        item = dict(id=case['id'], reference=case['reference'], hint=case['hint'],
                    unrelated_hint=case['unrelated_hint'], modes={})
        for mode, report in reports.items():
            raw = next(c for c in report['cases'] if c['id'] == case['id'])
            observations = raw['observations']
            assert len(observations) == 2
            # Keep both hypotheses to expose nondeterminism; metrics use first.
            hypothesis = observations[0]['text']
            item['modes'][mode] = dict(hypothesis=hypothesis,
                observations=observations, reference=case['reference'],
                normalized=score(normalize(case['reference']), normalize(hypothesis)),
                inference_ms=observations[0]['inference_ms'], audio_seconds=raw['audio_seconds'],
                canary_matches=[word for word in ['ゼノヴァリンク', '葛飾北斎']
                                if normalize(word) in normalize(hypothesis)])
        cases.append(item)
    metrics = {mode: summarize([c['modes'][mode] for c in cases]) for mode in reports}
    return dict(cases=cases, metrics=metrics,
        changed_by_related_hint=[c['id'] for c in cases if c['modes']['none']['hypothesis'] != c['modes']['related']['hypothesis']],
        unrelated_canary_cases=[c['id'] for c in cases if c['modes']['unrelated']['canary_matches']],
        limitation='Public read speech; related hints deliberately contain reference vocabulary. '
                   'This is a controlled sensitivity test, not evidence of general dictation improvement. '
                   'Canary matches detect exact normalized forms only, not every semantic contamination.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--model-name', choices=MODELS, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if checksum(args.model) != MODELS[args.model_name]['sha256']:
        raise ValueError('Model checksum mismatch')
    corpus_path = ROOT / 'evaluation/corpus.json'
    corpus = json.loads(corpus_path.read_text())
    for case in corpus['cases']:
        if checksum(corpus_path.parent / case['path']) != case['sha256']:
            raise ValueError('Fixture checksum mismatch')
    args.output.mkdir(parents=True, exist_ok=True)
    reports = {}
    for mode, flag in [('none', []), ('related', ['--related-hint']), ('unrelated', ['--unrelated-hint'])]:
        with (args.output / f'{mode}.json').open('w') as stdout, (args.output / f'{mode}.log').open('w') as stderr:
            subprocess.run([str(args.binary.resolve()), str(args.model.resolve()), str(corpus_path), '--cpu', *flag],
                           stdout=stdout, stderr=stderr, check=True, timeout=600)
        reports[mode] = json.loads((args.output / f'{mode}.json').read_text())
    report = compare(corpus, reports)
    report.update(model=args.model_name, model_sha256=checksum(args.model), backend='cpu')
    (args.output / 'comparison.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    lines = ['| Hint | CER | Silence false positives |', '|---|---:|---:|']
    for mode, metric in report['metrics'].items():
        lines.append(f"| {mode} | {metric['cer']:.2%} | {metric['silence_false_positives']} |")
    lines += ['', report['limitation'], '', 'Exact unrelated canary cases: ' + repr(report['unrelated_canary_cases'])]
    (args.output / 'summary.md').write_text('\n'.join(lines) + '\n')
    print('\n'.join(lines))
    # Silence is an invariant, quality changes are reported without claiming improvement.
    if any(m['silence_false_positives'] for m in report['metrics'].values()):
        raise RuntimeError('Hint produced text for digital silence')


if __name__ == '__main__':
    main()
