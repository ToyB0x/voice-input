#!/usr/bin/env python3
"""Exercise the app's exact resident prompts against a real local llama server.
CPU quality evidence only: a generous inference deadline is deliberate.
"""
import argparse,base64,json,subprocess,time,urllib.request,urllib.error,socket,secrets,hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser()
    for key in ['binary','models','protocol','output']:p.add_argument('--'+key,type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    config=json.loads((ROOT/'evaluation/ame-lt/scenarios.json').read_text())
    spec=json.loads((ROOT/'evaluation/local-vlm/model.json').read_text())
    paths={}
    for entry in spec['files']:
        path=a.models/entry['file']
        if hashlib.file_digest(path.open('rb'),'sha256').hexdigest()!=entry['sha256']:raise ValueError('Model checksum mismatch')
        paths[entry['role']]=path
    def protocol(raw='雨がすごいですね',text='手がかりはありません。',**extra):
        return json.loads(subprocess.check_output([a.protocol],input=json.dumps(dict(raw=raw,text=text,resident=True,**extra),ensure_ascii=False).encode()))
    contract=protocol()
    token=secrets.token_hex(32)
    with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    url=f'http://127.0.0.1:{port}'
    opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def request(path,body=None):
        req=urllib.request.Request(url+path,data=None if body is None else json.dumps(body).encode(),
            headers={'Content-Type':'application/json','Authorization':'Bearer '+token})
        with opener.open(req,timeout=1800) as r:return json.load(r)
    report=dict(complete=False,quality_pass=False,backend='cpu',network_denied=False,
        source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        timing_scope='resident-model-inference-only; excludes ASR; no 3-second deadline in CPU quality test',
        model=spec,observations=[],rows=[])
    def save():(a.output/'resident.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    save();start=time.monotonic()
    with (a.output/'server.log').open('w') as log:
        child=subprocess.Popen([str(a.binary),'-m',str(paths['model']),'--mmproj',str(paths['projector']),
            '--host','127.0.0.1','--port',str(port),'--api-key',token,
            '-c','8192','--parallel','1','--threads','4','--image-max-tokens','4096',
            '-ngl','0','--device','none','--no-op-offload','--no-mmproj-offload','--no-webui'],
            stdin=subprocess.DEVNULL,stdout=log,stderr=log)
        try:
            deadline=time.monotonic()+300
            while True:
                if child.poll() is not None or time.monotonic()>deadline:raise RuntimeError('Server did not start')
                try:request('/health');break
                except (urllib.error.URLError,TimeoutError):time.sleep(.2)
            for case in config['images']:
                data=(ROOT/case['file']).read_bytes()
                if hashlib.sha256(data).hexdigest()!=case['sha256']:raise ValueError('Image checksum mismatch')
                body=contract['residentBody'].copy()
                body['messages']=[{'role':'user','content':[
                    {'type':'text','text':contract['observationPrompt']},
                    {'type':'image_url','image_url':{'url':'data:image/png;base64,'+base64.b64encode(data).decode()}}]}]
                body['response_format']['json_schema']['schema']=contract['observationSchema']
                value=json.loads(request('/v1/chat/completions',body)['choices'][0]['message']['content'])
                observation=value['observation']
                if not isinstance(observation,str) or not 1<=len(observation)<=160:raise ValueError('Invalid observation')
                report['observations'].append(observation);save()
            request('/v1/chat/completions',protocol()['residentBody'])
            report['preparation_ms']=round((time.monotonic()-start)*1000)
            cases=[('text',c,c['text']) for c in config['texts']]+[('image',c,o) for c,o in zip(config['images'],report['observations'])]
            for repeat in range(2):
                for stage,case,context in (cases if repeat==0 else reversed(cases)):
                    contract=protocol(text=context)
                    start=time.monotonic()
                    response=request('/v1/chat/completions',contract['residentBody'])
                    elapsed=round((time.monotonic()-start)*1000)
                    output=response['choices'][0]['message']['content']
                    report['pending_output']=output;save()
                    parsed=protocol(text=context,output=output)
                    report.pop('pending_output',None)
                    expected=case.get('expected',case.get('target'))
                    report['rows'].append(dict(stage=stage,id=case['id'],repeat=repeat,
                        context=context,prompt=contract['prompt'],output=output,final=parsed['final'],
                        expected=expected,passed=parsed['final']==expected+'がすごいですね',elapsed_ms=elapsed))
                    save()
            report['complete']=True;report['quality_pass']=all(r['passed'] for r in report['rows']);save()
        finally:
            child.kill();child.wait()
    print(json.dumps(dict(quality_pass=report['quality_pass'],rows=len(report['rows']))))
    if not report['quality_pass']:raise SystemExit(1)
if __name__=='__main__':main()
