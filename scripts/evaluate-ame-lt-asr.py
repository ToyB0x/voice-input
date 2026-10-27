#!/usr/bin/env python3
"""Actually transcribe the same public WAV twice, with an empty hint."""
import argparse,json,platform,socket,struct,subprocess,wave
from pathlib import Path
from ame_lt import ROOT,config,validate_asr
from transcription_models import MODELS,checksum
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--model',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
try:socket.create_connection(('127.0.0.1',9),timeout=1)
except PermissionError:pass
else:raise ValueError('Network denial not enforced')
c=config();audio=ROOT/c['audio']
if checksum(audio)!=c['audio_sha256'] or checksum(a.model)!=MODELS['large-v3']['sha256']:raise ValueError('Wrong audio or model')
with wave.open(str(audio),'rb') as w:
    if (w.getnchannels(),w.getsampwidth(),w.getframerate())!=(1,2,16000):raise ValueError('Unexpected WAV format')
    pcm=w.readframes(w.getnframes())
samples=b''.join(struct.pack('<f',x/32768) for (x,) in struct.iter_unpack('<h',pcm))
header=json.dumps({'samples':len(samples)//4,'hint':''}).encode()+b'\n'
a.output.mkdir(parents=True,exist_ok=True)
report=dict(complete=False,source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
    audio_sha256=checksum(audio),audio_ms=len(samples)//4*1000//16000,utterance=c['utterance'],
    synthetic_voice='Eddy (Japanese (Japan)); fixed macOS system-voice recording',
    model='Whisper large-v3',model_sha256=checksum(a.model),binary_sha256=checksum(a.binary),
    backend='cpu',network_denied=True,hint='',platform=platform.platform(),rows=[])
for repeat in range(2):
    result=subprocess.run([str(a.binary.resolve()),str(a.model.resolve()),'cpu'],input=header+samples,capture_output=True,timeout=600)
    if result.returncode:raise ValueError('ASR runtime failed')
    row=json.loads(result.stdout);validate_asr(row['text'])
    report['rows'].append(dict(repeat=repeat,result=row));(a.output/'asr.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
report['complete']=True
(a.output/'asr.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print([r['result']['text'] for r in report['rows']])
