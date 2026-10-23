#!/usr/bin/env python3
"""Verify a built CLI offline. Writes only public fixture text/timings, never audio."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import wave

parser = argparse.ArgumentParser()
parser.add_argument('--binary', required=True, type=Path)
parser.add_argument('--model', required=True, type=Path)
parser.add_argument('--report', required=True, type=Path)
parser.add_argument('--cpu', action='store_true')
args = parser.parse_args()
fixture = Path(__file__).resolve().parent.parent / 'crates/local-transcription/tests/fixtures/ja-oku.wav'


def run(model, audio, success):
    command = [str(args.binary.resolve()), str(model), str(audio)]
    if args.cpu:
        command.append('--cpu')
    result = subprocess.run(command, capture_output=True, text=True, timeout=180)
    if (result.returncode == 0) != success:
        raise AssertionError(f'Unexpected exit {result.returncode}: {result.stderr}')
    return result


japanese = run(args.model, fixture, True)
args.report.parent.mkdir(parents=True, exist_ok=True)
args.report.with_suffix('.diagnostics.log').write_text(japanese.stderr)
report = json.loads(japanese.stdout)
for word in ['月日', '旅']:
    assert word in report['text'], report
with tempfile.TemporaryDirectory(prefix='voice-input-asr-') as directory:
    root = Path(directory)
    silence = root / 'silence.wav'
    with wave.open(str(silence), 'wb') as audio:
        audio.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        audio.writeframes(bytes(32000 * 2))
    assert json.loads(run(args.model, silence, True).stdout)['text'] == ''
    missing = run(root / 'missing.bin', fixture, False)
    assert 'model file is missing or unreadable' in missing.stderr
    invalid = root / 'invalid.wav'
    invalid.write_bytes(b'not a WAV')
    assert 'could not read WAV' in run(args.model, invalid, False).stderr
    corrupt = root / 'corrupt.bin'
    corrupt.write_bytes(b'not a Whisper model')
    assert 'could not load local model' in run(corrupt, fixture, False).stderr
    wrong_rate = root / 'wrong-rate.wav'
    with wave.open(str(wrong_rate), 'wb') as audio:
        audio.setparams((1, 2, 48000, 0, 'NONE', 'not compressed'))
        audio.writeframes(bytes(48000 * 2))
    assert 'WAV must be' in run(args.model, wrong_rate, False).stderr
report['checks'] = ['japanese', 'silence', 'missing_model', 'invalid_wav', 'corrupt_model', 'unsupported_rate']
args.report.parent.mkdir(parents=True, exist_ok=True)
args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
print(json.dumps(report, ensure_ascii=False, indent=2))
