"""Explicit lexical annotations, independent of scene IDs and ASR answers.

This does not infer synonyms. The caller owns the correctness of each mapping.
Raw observations must be retained separately from the annotated context.
"""
import json
import re


def load_lexicon(path):
    if path.stat().st_size > 64_000:
        raise ValueError('Lexicon exceeds bounds')
    entries = json.loads(path.read_text())
    if not isinstance(entries, list) or len(entries) > 128:
        raise ValueError('Invalid lexicon')
    aliases = {}
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {'term', 'canonical'}:
            raise ValueError('Invalid lexical entry')
        term, canonical = entry['term'], entry['canonical']
        if any(not isinstance(s, str) or not 1 <= len(s) <= 40
               or not re.fullmatch(r'[\u3041-\u3096\u30a1-\u30fa\u3400-\u9fffー]+', s)
               for s in [term, canonical]):
            raise ValueError('Invalid lexical spelling')
        if term in aliases or term == canonical:
            raise ValueError('Duplicate or identity alias')
        aliases[term] = canonical
    if set(aliases) & set(aliases.values()):
        raise ValueError('Chained aliases are not supported')
    return aliases


def annotate(text, aliases):
    if not aliases:
        return text
    # One pass, longest match first: no recursive expansion or partial overlap.
    pattern = '|'.join(re.escape(term) for term in sorted(aliases, key=len, reverse=True))
    return re.sub(pattern, lambda match: match[0] + '（' + aliases[match[0]] + '）', text)
