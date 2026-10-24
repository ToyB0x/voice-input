#!/usr/bin/env python3
"""Rebuild committed public fixtures from the pinned FLEURS parquet (setup only).
Requires pyarrow and ffmpeg. No inference, no user recordings, no network calls.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import wave

ROOT = Path(__file__).resolve().parent.parent
PARQUET_SHA256 = "4636065a2eb7d1bb07007d3193092eb1856cf101360831c61d6dfa611e579071"
SELECTION = [(2, "daily_walk"), (59, "daily_device"), (104, "request"),
             (234, "work_report"), (192, "business"),
             (27, "proper_nouns_japan"), (243, "proper_nouns_person")]
REVISION = "168de341b3db6859a9bac1c50a2ef5e3b47647e0"


def main():
    import pyarrow.parquet as pq
    if len(sys.argv) != 2:
        raise SystemExit("usage: prepare-evaluation-corpus.py PINNED_PARQUET")
    source = Path(sys.argv[1])
    digest = hashlib.sha256()
    with source.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    if digest.hexdigest() != PARQUET_SHA256:
        raise SystemExit("Unexpected parquet checksum")
    rows = pq.read_table(source).to_pylist()
    cases = []
    for index, category in SELECTION:
        row = rows[index]
        original = row['audio']['bytes']
        pcm = subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', 'pipe:0',
                              '-ar', '16000', '-ac', '1', '-f', 's16le', 'pipe:1'],
                             input=original, capture_output=True, check=True).stdout
        dest = ROOT / f'evaluation/fixtures/fleurs-ja-{index:03}.wav'
        with wave.open(str(dest), 'wb') as audio:
            audio.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
            audio.writeframes(pcm)
        cases.append(dict(id=f'fleurs-ja-{index:03}', path=f'fixtures/{dest.name}', category=category,
                          reference=row['raw_transcription'], sha256=hashlib.sha256(dest.read_bytes()).hexdigest(),
                          source=dict(dataset='google/fleurs', revision=REVISION, config='ja_jp', split='validation',
                                      row=index, sentence_id=row['id'], audio_path=row['audio']['path'],
                                      gender_label=row['gender'], original_sha256=hashlib.sha256(original).hexdigest())))
    dest = ROOT / 'evaluation/fixtures/silence.wav'
    with wave.open(str(dest), 'wb') as audio:
        audio.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        audio.writeframes(bytes(2 * 16000 * 2))
    cases.append(dict(id='silence', path=f'fixtures/{dest.name}', category='silence', reference='',
                      sha256=hashlib.sha256(dest.read_bytes()).hexdigest(), source=dict(generator='2 seconds digital zero PCM')))
    (ROOT / 'evaluation/corpus.json').write_text(json.dumps(dict(schema_version=1, cases=cases), ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    main()
