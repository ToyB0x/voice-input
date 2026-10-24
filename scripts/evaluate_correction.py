#!/usr/bin/env python3
"""Offline four-way correction comparison. Public/fixed contexts, not live OCR accuracy."""
import argparse,json,os,platform,re,socket,subprocess,time
from pathlib import Path
from correction_models import MODELS
from transcription_models import checksum
from evaluate_models import ROOT,normalize,score

MODES={'none':(False,False),'asr_only':(True,False),'correction_only':(False,True),'both':(True,True)}

def constraints(text,case):
    return dict(missing=[word for word in case.get('required',[]) if word not in text],
                invented=[word for word in case.get('forbidden',[]) if word in text],
                format_ok=(not case.get('line_break') or '\n' in text) and (not case.get('punctuation') or text.endswith(('。','！','？'))))

def infer(binary,model,backend,raw,context):
    command=[str(binary.resolve()),str(model.resolve()),backend]
    if platform.system()=='Darwin':command=['/usr/bin/time','-l',*command]
    started=time.monotonic()
    result=subprocess.run(command,input=json.dumps(dict(transcript=raw,context=context),ensure_ascii=False),capture_output=True,text=True,timeout=90)
    memory=re.search(r'(\d+)\s+maximum resident set size',result.stderr)
    item=dict(wall_ms=(time.monotonic()-started)*1000,peak_rss_bytes=int(memory[1]) if memory else None,rejected=False,failed=result.returncode!=0)
    if result.returncode:
        # A rejected preview preserves raw ASR in the actual app. Include both
        # rejection count and fallback text; never present fallback as a correction.
        try:failure=json.loads(result.stdout)
        except (ValueError,TypeError):failure={"error":"runtime","stage":"unknown"}
        item.update(text=raw,model_report=None,rejected=failure.get('error')=='rejected',failure=failure)
    else:
        report=json.loads(result.stdout)
        if report['backend']!=backend or (backend=='metal' and raw and report['offloaded_layers']<1) or (backend=='cpu' and report['offloaded_layers']!=0):
            raise ValueError('Requested correction backend was not actually used')
        item.update(text=report['text'],model_report=report)
    return item

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--model',type=Path,required=True)
    parser.add_argument('--model-name',choices=MODELS,required=True)
    parser.add_argument('--asr',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--require-network-denied',action='store_true')
    args=parser.parse_args()
    if args.require_network_denied:
        try:socket.create_connection(('127.0.0.1',9),timeout=1)
        except PermissionError:pass
        else:raise ValueError('Network sandbox was not enforced')
    if checksum(args.model)!=MODELS[args.model_name]['sha256']:raise ValueError('Correction model checksum mismatch')
    corpus=json.loads((ROOT/'evaluation/corpus.json').read_text())
    asr={mode:json.loads((args.asr/(mode+'.json')).read_text()) for mode in ['none','related']}
    expected=[case['id'] for case in corpus['cases']]
    for report in asr.values():
        if report['model_name']!='small' or [case['id'] for case in report['cases']]!=expected:raise ValueError('Incomplete or wrong ASR evaluation input')
    args.output.mkdir(parents=True,exist_ok=True)
    # Hardware support is a prerequisite, so keep a safe standalone failure
    # report before spending time on the full four-route CPU comparison.
    gpu=infer(args.binary,args.model,'metal','会議は金曜日です','会議、金曜日。')
    (args.output/'metal-smoke.json').write_text(json.dumps(gpu,ensure_ascii=False,indent=2)+'\n')
    if gpu['failed']:raise ValueError('Actual Metal inference failed; inspect metal-smoke.json for the fixed failure stage')
    records=[]
    for index,case in enumerate(corpus['cases']):
        for mode,(asr_context,correction_context) in MODES.items():
            raw=asr['related' if asr_context else 'none']['cases'][index]['observations'][0]['text']
            item=infer(args.binary,args.model,'cpu',raw,case['hint'] if correction_context else '')
            item.update(id=case['id'],mode=mode,raw_asr=raw,reference=case['reference'],context=case['hint'],
                        raw_score=score(normalize(case['reference']),normalize(raw)),
                        final_score=score(normalize(case['reference']),normalize(item['text'])))
            records.append(item)
            print(case['id'],mode,'rejected' if item['rejected'] else ('runtime-failed' if item['failed'] else 'accepted'),flush=True)
    supplemental=[]
    for case in json.loads((ROOT/'evaluation/correction-cases.json').read_text())['cases']:
        item=infer(args.binary,args.model,'cpu',case['transcript'],case['context'])
        item.update(case=case,constraints=constraints(item['text'],case))
        supplemental.append(item)
        if not case['transcript'] and item['text']:raise ValueError('Correction hallucinated on empty input')
    summaries={}
    for mode in MODES:
        rows=[r for r in records if r['mode']==mode]
        chars=sum(r['final_score']['reference_chars'] for r in rows)
        spoken=[r for r in rows if r['reference']]
        summaries[mode]=dict(raw_cer=sum(r['raw_score']['edits'] for r in rows if r['reference'])/chars,cer=sum(r['final_score']['edits'] for r in rows if r['reference'])/chars,
            rejected=sum(r['rejected'] for r in rows),runtime_failures=sum(r['failed'] and not r['rejected'] for r in rows),mean_wall_ms=sum(r['wall_ms'] for r in spoken)/len(spoken),
            peak_rss_bytes=max((r['peak_rss_bytes'] or 0) for r in rows),
            silence_false_positives=sum(bool(r['text']) for r in rows if not r['reference']))
    report=dict(model=args.model_name,model_identity=MODELS[args.model_name],source_revision=os.environ.get('GITHUB_SHA'),
        network_denied=args.require_network_denied,asr_input_sha256={mode:checksum(args.asr/(mode+'.json')) for mode in asr},
        corpus_sha256=checksum(ROOT/'evaluation/corpus.json'),supplemental_sha256=checksum(ROOT/'evaluation/correction-cases.json'),
        summary=summaries,records=records,supplemental=supplemental,metal_smoke=gpu,
        limitation='Fixed reference-vocabulary contexts and public read speech. Real screen OCR integration is tested separately. '
                   'Constraint checks cover only listed facts/negations/formatting; they do not establish general semantic equivalence. '
                   'Rejected previews retain raw ASR and are counted separately. VM timings do not predict physical Mac performance.')
    (args.output/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    lines=['| Context routing | Raw ASR CER | Final CER | Rejected previews | Runtime failures | Mean spoken ms | Peak MiB |','|---|---:|---:|---:|---:|---:|---:|']
    for mode,item in summaries.items():lines.append(f"| {mode} | {item['raw_cer']:.2%} | {item['cer']:.2%} | {item['rejected']} | {item['runtime_failures']} | {item['mean_wall_ms']:.0f} | {item['peak_rss_bytes']/1048576:.1f} |")
    lines+=['',report['limitation'],'','Supplemental constraints:']
    for item in supplemental:lines.append(f"- {item['case']['id']}: rejected={item['rejected']}, {item['constraints']}")
    (args.output/'summary.md').write_text('\n'.join(lines)+'\n')
    print('\n'.join(lines))
    if any(item['failed'] and not item['rejected'] for item in records+supplemental):raise ValueError('Correction runtime failed; see report.json for safe failure stages')
    if any(item['silence_false_positives'] for item in summaries.values()):raise ValueError('Silence invariant failed')

if __name__=='__main__':main()
