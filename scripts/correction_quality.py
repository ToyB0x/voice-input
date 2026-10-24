"""Fixed-case quality checks and human-readable diffs, not a semantic verifier."""
import difflib
import json
import re
from pathlib import Path


def constraints(text, case):
    missing = [word for word in case.get('required', []) if word not in text]
    missing += ['one-of:'+'|'.join(group) for group in case.get('required_any', []) if not any(word in text for word in group)]
    invented = [word for word in case.get('forbidden', []) if word in text]
    violations = []
    if case.get('line_break') and '\n' not in text:
        violations.append('line-break')
    if case.get('punctuation') and not text.endswith(('。', '！', '？')):
        violations.append('sentence-ending')
    if case.get('bullets'):
        lines = [line for line in text.splitlines() if line.strip()]
        if len(lines) != case['bullets'] or not all(line.startswith('- ') and line[2:].strip() for line in lines):
            violations.append('bullet-layout')
    if case.get('paragraphs') and len([p for p in text.split('\n\n') if p.strip()]) != case['paragraphs']:
        violations.append('paragraph-count')
    violations += ['retained-format:' + word for word in case.get('forbidden_format', []) if word in text]
    return dict(missing=missing, invented=invented, facts_ok=not missing and not invented,
                format_ok=not violations, format_violations=violations)


def load_cases(path):
    cases = json.loads(Path(path).read_text())['cases']
    ids = [case['id'] for case in cases]
    if not cases or len(ids) != len(set(ids)) or any(not re.fullmatch('[a-z0-9-]{1,80}', value) for value in ids):
        raise ValueError('Invalid or duplicate correction case IDs')
    for case in cases:
        if any(not isinstance(case.get(key), str) for key in ['transcript', 'context', 'reference']):
            raise ValueError('Correction cases require fixed input, context and reference')
        for key in ['required', 'forbidden', 'forbidden_format']:
            if not isinstance(case.get(key, []), list) or any(not isinstance(word, str) or not word for word in case.get(key, [])):
                raise ValueError('Invalid correction constraint')
        if not isinstance(case.get('required_any', []), list) or any(not isinstance(group,list) or not group or any(not isinstance(word,str) or not word for word in group) for group in case.get('required_any', [])):
            raise ValueError('Invalid alternative constraint')
        for key in ['bullets', 'paragraphs']:
            if key in case and (type(case[key]) is not int or case[key] < 1):
                raise ValueError('Invalid format count')
    return cases


def quality_summary(items):
    generated = [item for item in items if item['case']['transcript'] and not item['failed'] and not item['rejected']]
    final = [item for item in items if item['case']['transcript']]
    return dict(final_checked=len(final), final_facts_passed=sum(item['constraints']['facts_ok'] for item in final),
                missing_required_cases=sum(bool(item['constraints']['missing']) for item in final),
                forbidden_cases=sum(bool(item['constraints']['invented']) for item in final),
                completed=len(items), generated=len(generated),
                rejected=sum(item['rejected'] for item in items),
                runtime_failures=sum(item['failed'] and not item['rejected'] for item in items),
                facts_passed=sum(item['constraints']['facts_ok'] for item in generated),
                format_passed=sum(item['constraints']['format_ok'] for item in generated),
                both_passed=sum(item['constraints']['facts_ok'] and item['constraints']['format_ok'] for item in generated),
                silence_false_positives=sum(bool(item['text']) for item in items if not item['case']['transcript']))


def diff_text(before, after, label):
    def lines(value):
        return [line + '\n' for line in value.splitlines()]
    result = ''.join(difflib.unified_diff(lines(before), lines(after),
                                       fromfile=label + '/before', tofile=label + '/after'))
    return result or label + ': unchanged\n'


def load_baseline(path, model, identity, cases):
    baseline = json.loads(Path(path).read_text())
    if baseline.get('schema_version') != 1 or baseline.get('model') != model or baseline.get('model_identity') != identity:
        raise ValueError('Baseline model identity mismatch')
    indexed = {case['id']: case for case in cases}
    seen = set()
    for row in baseline['cases']:
        case = row['case']
        if case['id'] in seen or case['id'] not in indexed:
            raise ValueError('Missing or duplicate baseline case')
        seen.add(case['id'])
        if any(case[key] != indexed[case['id']][key] for key in ['transcript', 'context', 'reference']):
            raise ValueError('Baseline inputs changed; comparison is not controlled')
        if not isinstance(row['result']['text'], str) or any(type(row['result'][k]) is not bool for k in ['failed', 'rejected']):
            raise ValueError('Invalid baseline outcome')
    if not seen:
        raise ValueError('Empty baseline')
    return baseline


def compare_baseline(baseline, items):
    indexed = {item['case']['id']: item for item in items}
    pairs, before_items, after_items = [], [], []
    for row in baseline['cases']:
        case = row['case']
        if case['id'] not in indexed:
            raise ValueError('Incomplete evaluation of baseline cases')
        after = indexed[case['id']]
        # Re-score old outputs with today's declared checks. Added cases are
        # excluded; neither the old labels nor a changed denominator imply gain.
        before = dict(row['result'], case=after['case'], constraints=constraints(row['result']['text'], after['case']))
        before_items.append(before)
        after_items.append(after)
        pairs.append(dict(id=case['id'], before=before, after=after,
                          diff=diff_text(before['text'], after['text'], 'baseline/' + case['id'])))
    return dict(source_revision=baseline['source_revision'], run_url=baseline['run_url'],
                before=quality_summary(before_items), after=quality_summary(after_items), pairs=pairs,
                limitation='Same model and identical original synthetic inputs; current checks applied to both outputs. '
                           'Frozen prior run, not a concurrent randomized trial or performance comparison.')
