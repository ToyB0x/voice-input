#!/usr/bin/env python3
"""Measured LT-only bounded choice: audio alone, audio+text, audio+image."""
import argparse,json,platform,re,socket,subprocess,time
from pathlib import Path
from ame_lt import ROOT,SCHEMA,config,prompt_for,parse_runtime_choice,apply_spelling,normalized,validate_asr
from transcription_models import checksum
from evaluate_correction import run_inference
def main():
    p=argparse.ArgumentParser()
    for name in ['binary','models','source','asr','output']:p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args()
    try:socket.create_connection(('127.0.0.1',9),timeout=1)
    except PermissionError:pass
    else:raise ValueError('Network denial not enforced')
    c=config();asr=json.loads(a.asr.read_text())
    if not asr['complete'] or asr['hint']!='' or asr['audio_sha256']!=c['audio_sha256'] or [r['repeat'] for r in asr['rows']]!=[0,1]:
        raise ValueError('Unverified context-free ASR')
    for r in asr['rows']:validate_asr(r['result']['text'])
    spec=json.loads((ROOT/'evaluation/local-vlm/model.json').read_text())
    if subprocess.check_output(['git','-C',str(a.source),'rev-parse','HEAD'],text=True).strip()!=spec['runtime']['revision']:raise ValueError('Wrong runtime')
    text_binary=a.binary.resolve().with_name('llama-completion')
    if not text_binary.is_file():raise ValueError('Missing text runtime')
    paths={}
    for entry in spec['files']:
        path=(a.models/entry['file']).resolve()
        if path.stat().st_size!=entry['bytes'] or checksum(path)!=entry['sha256']:raise ValueError('Wrong model')
        paths[entry['role']]=path
    for case in c['images']:
        if checksum(ROOT/case['file'])!=case['sha256']:raise ValueError('Wrong image')
    a.output.mkdir(parents=True,exist_ok=True)
    report=dict(complete=False,kind='ame-lt-three-stages',source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        config=c,config_sha256=checksum(ROOT/'evaluation/ame-lt/scenarios.json'),asr=asr,asr_report_sha256=checksum(a.asr),
        model=spec,binary_sha256=checksum(a.binary),text_binary_sha256=checksum(text_binary),backend='cpu',network_denied=True,platform=platform.platform(),
        output_schema=SCHEMA,rows=[],limitation='LT-only rain/candy choice on one reused synthetic audio and constructed helpful contexts. Not general dictation accuracy or acoustic accent recognition. A text/vision model receives ASR text, not audio bytes. No OCR or authored caption is used in image mode.')
    def save():(a.output/'results.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    save()
    for repeat in range(2):
        raw=asr['rows'][repeat]['result']['text']
        report['rows'].append(dict(stage='audio',repeat=repeat,id='no-context',audio_sha256=c['audio_sha256'],raw=raw,final=raw,status='completed',context='',image=None,model_called=False))
        cases=[('text',x) for x in c['texts']]+[('image',x) for x in c['images']]
        for stage,case in cases if repeat==0 else reversed(cases):
            prompt=prompt_for(raw,stage,case);image_copy=a.output/'input.png'
            command=[str(a.binary.resolve()),'-m',str(paths['model']),'--mmproj',str(paths['projector']),
                '-c','8192','-n','128','--temp','0','--seed','0','--json-schema',json.dumps(SCHEMA),
                '--threads','4','--image-max-tokens','4096','--no-warmup','-ngl','0','--no-mmproj-offload']
            if stage=='image':
                image_copy.write_bytes((ROOT/case['file']).read_bytes());command+=['--image',str(image_copy.resolve()),'-p',prompt]
            else:
                command=[str(text_binary),'-m',str(paths['model']),'-cnv','-st','--simple-io','--no-display-prompt',
                    '-c','8192','-n','128','--temp','0','--seed','0','--json-schema',json.dumps(SCHEMA),
                    '--threads','4','--no-warmup','-ngl','0','-p',prompt]
            row=dict(stage=stage,id=case['id'],repeat=repeat,audio_sha256=c['audio_sha256'],raw=raw,
                context=case['text'] if stage=='text' else '',image=case.get('file'),image_sha256=case.get('sha256'),
                prompt=prompt,expected=c['utterance'].replace('あめ',case['expected']),model_called=True,status='runtime-error')
            start=time.monotonic()
            try:
                result=run_inference(command,'',timeout=1800)
                (a.output/f"{stage}-{case['id']}-{repeat}.stderr.txt").write_text(result.stderr)
                row.update(model_output=result.stdout,exit_status=result.returncode)
                if result.returncode:raise ValueError('Model runtime failure')
                if any(int(n)>0 for n in re.findall(r'offloaded (\d+)/\d+ layers to GPU',result.stderr)):raise ValueError('Unexpected GPU')
                spelling=parse_runtime_choice(result.stdout,stage);final=apply_spelling(raw,spelling)
                row.update(status='completed',spelling=spelling,final=final,passed=spelling!='不明' and normalized(final)==row['expected'])
            except (ValueError,subprocess.TimeoutExpired) as e:row['error']=str(e)[:300]
            finally:image_copy.unlink(missing_ok=True)
            row['wall_ms']=round((time.monotonic()-start)*1000);report['rows'].append(row);save()
            print(stage,case['id'],row.get('final',row.get('error')),flush=True)
    report['complete']=len(report['rows'])==10
    report['runtime_pass']=report['complete'] and all(r['status']=='completed' for r in report['rows'])
    report['quality_pass']=report['runtime_pass'] and all(r.get('passed',True) for r in report['rows'])
    report['stable']=all(len({r.get('final') for r in report['rows'] if r['stage']==stage and r['id']==key})==1 for stage,key in [('audio','no-context')]+[(s,x['id']) for s,k in [('text','texts'),('image','images')] for x in c[k]])
    report['quality_pass']=report['quality_pass'] and report['stable'];save()
    if not report['runtime_pass']:raise SystemExit(1)
if __name__=='__main__':main()
