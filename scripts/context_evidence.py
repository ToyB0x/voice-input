#!/usr/bin/env python3
"""Actual local inference on synthetic public fixtures. References never enter model input."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import socket
import struct
import subprocess
import time
import unicodedata
import wave
from correction_models import MODELS as CORRECTION_MODELS
from transcription_models import MODELS as SPEECH_MODELS

ROOT = Path(__file__).resolve().parents[1]
KANJI = '文脈から判断できる同音語は自然な漢字表記にしてください。判断できない場合は推測せず元の表記を保ってください。'
SCENES = {'storm': ('雨', '飴'), 'candy': ('飴', '雨')}

def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''): h.update(block)
    return h.hexdigest()

def recognition_hint(text):
    # Same byte cap and control-character removal as the product.
    result=''
    for c in text:
        if unicodedata.category(c)=='Cc': continue
        if len((result+c).encode())>768: break
        result+=c
    return result.strip()

def judge(text, expected, opposite):
    if expected in text and opposite not in text: return 'target'
    if opposite in text and expected not in text: return 'opposite'
    if expected in text and opposite in text: return 'mixed'
    return 'undetermined'  # kana / omitted / unrelated; never award a correct guess

def sources(capture, other):
    labels = capture['vision'].get('labels', [])
    return {
        'fixture-text': capture['fixtureText'],  # text injection CONTROL, never called AX
        'ax': capture['ax'].get('text', ''),
        'ocr': capture['ocr'].get('text', ''),
        'image-ocr': capture['imageOCR'].get('text', ''),
        'vision-labels': '\n'.join(f"{x['label']} ({x['confidence']:.3f})" for x in labels) if capture['vision']['status']=='completed' else '',
        'mismatched-text': other['fixtureText'],
    }

def run_json(command, payload=None, timeout=150):
    r = subprocess.run([str(x) for x in command], input=payload, capture_output=True, timeout=timeout)
    try: value=json.loads(r.stdout)
    except (ValueError, UnicodeError): return {'failure': 'invalid-json', 'returnCode': r.returncode}
    if r.returncode: value['failure']='runtime-failed'
    return value

class Resident:
    """One request at a time; helper creates a new KV context for every request."""
    def __init__(self, binary, model, log):
        self.log=log.open('wb')
        self.child=subprocess.Popen([str(binary), str(model), 'cpu', '--serve-compare'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log)
    def request(self, payload):
        self.child.stdin.write(payload+b'\n');self.child.stdin.flush()
        if not select.select([self.child.stdout], [], [], 150)[0]: raise TimeoutError('Correction helper exceeded 150 seconds')
        line=self.child.stdout.readline(24577)
        if not line or len(line)>24576: raise RuntimeError('Missing or oversized correction report')
        return json.loads(line)
    def close(self):
        try:
            self.child.stdin.close();self.child.wait(timeout=15)
        except (BrokenPipeError, subprocess.TimeoutExpired):
            self.child.kill();self.child.wait(timeout=15)
        self.log.close()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['fixtures','output','asr-binary','speech-model','correction-binary','correction-model','foundation-binary']:
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--repeats', type=int, choices=range(1,4), default=2)
    p.add_argument('--require-network-denied', action='store_true')
    a=p.parse_args()
    if a.require_network_denied:
        try: socket.create_connection(('127.0.0.1', 9), timeout=1)
        except PermissionError: pass
        else: raise RuntimeError('Network sandbox was not enforced')
    a.output.mkdir(parents=True,exist_ok=True)
    fixtures=json.loads((a.fixtures/'fixtures.json').read_text())
    for rel,sha in fixtures['hashes'].items():
        if digest(a.fixtures/rel)!=sha: raise RuntimeError('Fixture checksum mismatch')
    if digest(a.speech_model)!=SPEECH_MODELS['base']['sha256']: raise RuntimeError('Whisper checksum mismatch')
    if digest(a.correction_model)!=CORRECTION_MODELS['qwen3-1.7b']['sha256']: raise RuntimeError('Qwen checksum mismatch')
    captures={scene:json.loads((a.fixtures/scene/'capture.json').read_text()) for scene in SCENES}
    foundation=run_json([a.foundation_binary,'--status'])
    report=dict(schemaVersion=1,sourceRevision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        platform=platform.platform(),machine=platform.machine(),networkDenied=a.require_network_denied,
        fixture=fixtures,models=dict(speech='Whisper base',correction='Qwen3-1.7B-Q8_0',speechSha256=digest(a.speech_model),correctionSha256=digest(a.correction_model)),
        binaries={n:digest(getattr(a,n)) for n in ['asr_binary','correction_binary','foundation_binary']},
        instructions={'default':'','kanji':KANJI},foundation=foundation,captures=captures,rows=[],
        limits=['One synthetic Japanese utterance, two constructed contexts; repeated runs are not independent speech samples.',
          'The TTS pitch accent can favor one spelling. Context appropriateness is judged against a constructed scenario, not acoustic ground truth.',
          'Window PNGs are AppKit rendered fixtures, not ScreenCaptureKit captures of another app. AX API failures remain unavailable.',
          'Vision classification labels are passed as text. No image is passed to Whisper, Qwen or Foundation Models; no VLM was evaluated.',
          'Candidate guards preserve limited invariants; accepted is not a correctness or semantic equivalence score.',
          'Only target-word selection is scored. No global accuracy, cross-language ranking or Typeless quality claim is supported.'])
    with wave.open(str(a.fixtures/'speech.wav'),'rb') as w:
        pcm=w.readframes(w.getnframes());audio=b''.join(struct.pack('<f',x/32768) for (x,) in struct.iter_unpack('<h',pcm))
    asr_cache={}
    def asr(hint,repeat):
        # Cache only across conditions that have exactly the same input; report reuse.
        key=(hint,repeat)
        if key not in asr_cache:
            header=json.dumps({'samples':len(audio)//4,'hint':hint},ensure_ascii=False).encode()+b'\n'
            asr_cache[key]=run_json([a.asr_binary,a.speech_model,'cpu'],header+audio)
        return asr_cache[key]
    engines=['qwen3']+(['apple-foundation'] if foundation.get('availability')=='available' else [])
    unavailable_engines=[] if len(engines)==2 else [{'engine':'apple-foundation','status':'unavailable','reason':foundation.get('availability',foundation.get('failure','unknown'))}]
    report['unavailableEngines']=unavailable_engines
    resident=Resident(a.correction_binary.resolve(),a.correction_model.resolve(),a.output/'qwen-native.log')
    def save(): (a.output/'results.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    try:
        for repeat in range(a.repeats):
            # Reverse scene and source order on the second pass to expose order effects.
            scenes=list(SCENES);scenes=scenes if repeat%2==0 else scenes[::-1]
            for scene in scenes:
                expected,opposite=SCENES[scene]
                src=sources(captures[scene],captures['candy' if scene=='storm' else 'storm'])
                conditions=[('none','none','')]+[(source,route,context) for source,context in src.items() for route in ['hint','correction','both']]
                if repeat%2: conditions.reverse()
                for source,route,context in conditions:
                    if source!='none' and not context.strip():
                        report['rows'].append(dict(scene=scene,repeat=repeat,source=source,route=route,status='unavailable',reason='No captured context',context='',engine=None));save();continue
                    hint=recognition_hint(context) if route in ['hint','both'] else ''
                    speech=asr(hint,repeat)
                    if 'failure' in speech:
                        report['rows'].append(dict(scene=scene,repeat=repeat,source=source,route=route,status='asr-failed',speech=speech));save();continue
                    raw=speech['text']
                    base=dict(scene=scene,repeat=repeat,source=source,route=route,context=context,raw=raw,speech=speech,expected=expected,opposite=opposite,audioSha256=fixtures['hashes']['speech.wav'])
                    # Keep the actual raw ASR result separate from post-correction outputs.
                    report['rows'].append(dict(base,engine='none',instruction='none',status='uncorrected',finalText=raw,judgment=judge(raw,expected,opposite)))
                    for engine in engines:
                        for instruction_name,instruction in report['instructions'].items():
                            payload=json.dumps(dict(transcript=raw,context=context if route in ['correction','both'] else '',instruction=instruction),ensure_ascii=False).encode()
                            start=time.monotonic()
                            try:
                                if engine=='qwen3': result=resident.request(payload)
                                else:
                                    result=run_json([a.foundation_binary,'--correct'],payload)
                                    candidate=result.get('text')
                                    if candidate is not None:
                                        check=run_json([a.correction_binary,'--validate'],json.dumps(dict(transcript=raw,candidate=candidate),ensure_ascii=False).encode())
                                        result.update(candidate=candidate,accepted='failure' not in check,reason=check.get('failure'))
                                accepted=result.get('accepted') is True
                                final=result.get('candidate',raw) if accepted else raw
                                row=dict(base,engine=engine,instruction=instruction_name,status='accepted' if accepted else 'fallback',candidate=result.get('candidate'),finalText=final,
                                    correctionMs=round((time.monotonic()-start)*1000),correction=result,judgment=judge(final,expected,opposite))
                            except (BrokenPipeError,RuntimeError,TimeoutError,ValueError,subprocess.TimeoutExpired) as error:
                                # A dead stream cannot be safely resynchronized; preserve partial report and fail.
                                report['interrupted']=type(error).__name__;save();raise
                            report['rows'].append(row);save()
                    print(f'{repeat+1}/{a.repeats} {scene} {source} {route}: {raw}',flush=True)
        # Isolate kana-to-kanji ability without presenting it as ASR accuracy.
        report['kanaControls']=[]
        for scene in SCENES:
            for name,instruction in report['instructions'].items():
                payload=json.dumps(dict(transcript=fixtures['utterance'],context=captures[scene]['fixtureText'],instruction=instruction),ensure_ascii=False).encode()
                result=resident.request(payload)
                report['kanaControls'].append(dict(scene=scene,instruction=name,input=fixtures['utterance'],result=result))
        report['preservationControls']=[]
        for raw,markers in [
            ('明日の午後3時に会議室Bで10人の会議をします',['3','10','B']),
            ('この変更は本番には適用しないでください',['ない']),
            ('金曜日ではなく木曜日にお願いします',['金曜日','ではなく','木曜日']),
            ('田中さんへ資料を送ります',['田中']),
        ]:
            for name,instruction in report['instructions'].items():
                payload=json.dumps(dict(transcript=raw,context='',instruction=instruction),ensure_ascii=False).encode()
                result=resident.request(payload)
                final=result.get('candidate',raw) if result.get('accepted') else raw
                report['preservationControls'].append(dict(input=raw,instruction=name,result=result,
                    markers=markers,finalText=final,markersPreserved=all(x in final for x in markers)))
    finally: resident.close()
    if resident.child.returncode!=0: raise RuntimeError('Resident helper did not exit cleanly')
    report['completed']=True;save()
    print('Completed real context evidence; results.json is authoritative.')

if __name__=='__main__': main()
