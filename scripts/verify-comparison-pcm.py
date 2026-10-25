#!/usr/bin/env python3
"""Real offline ASR through the comparison PCM pipe; no temporary audio files."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import wave

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True);p.add_argument('--model',type=Path,required=True)
    p.add_argument('--backend',choices=['cpu','metal'],default='cpu');p.add_argument('--output',type=Path,required=True)
    args=p.parse_args()
    root=Path(__file__).resolve().parent.parent
    with wave.open(str(root/'evaluation/fixtures/fleurs-ja-002.wav')) as source:
        assert source.getnchannels()==1 and source.getsampwidth()==2 and source.getframerate()==16000
        raw=source.readframes(source.getnframes())
    samples=[sample[0]/32768 for sample in struct.iter_unpack('<h',raw)]
    def run(audio,hint=''):
        packet=json.dumps(dict(samples=len(audio),hint=hint),ensure_ascii=False).encode()+b'\n'+struct.pack(f'<{len(audio)}f',*audio)
        return subprocess.run([str(args.binary.resolve()),str(args.model.resolve()),args.backend],input=packet,capture_output=True,timeout=180)
    output=run(samples)
    if output.returncode:raise RuntimeError('PCM comparison ASR failed: '+output.stdout.decode())
    report=json.loads(output.stdout)
    assert report['backend']==args.backend and report['text'].strip() and not report['silent']
    if args.backend=='metal':assert report['metal_active'] is True
    silence=run([0.0]*16000)
    assert silence.returncode==0 and json.loads(silence.stdout)['text']==''
    invalid=run([float('nan')]*16000)
    assert invalid.returncode!=0 and json.loads(invalid.stdout)['error']=='invalid-pcm'
    # Validating input must precede any model load; no paths, hint or user input reflected in errors.
    assert b'NaN' not in invalid.stdout and str(args.model).encode() not in invalid.stdout
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(dict(status='passed',backend=args.backend,recognition=report,silenceBypassed=True,invalidPcmRejected=True),ensure_ascii=False,indent=2)+'\n')
if __name__=='__main__':main()
