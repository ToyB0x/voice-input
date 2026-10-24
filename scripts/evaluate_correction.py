#!/usr/bin/env python3
"""Offline four-way correction comparison. Public/fixed contexts, not live OCR accuracy."""
import argparse,json,os,platform,re,signal,socket,subprocess,time
from pathlib import Path
from correction_models import MODELS
from transcription_models import checksum
from evaluate_models import ROOT,normalize,score
from transcription_models import MODELS as ASR_MODELS
from correction_quality import constraints,load_cases,quality_summary,load_baseline,compare_baseline,diff_text

MODES={'none':(False,False),'asr_only':(True,False),'correction_only':(False,True),'both':(True,True)}

def metal_unavailable(failure):
    return (failure.get('error')=='runtime' and failure.get('stage')=='backend'
        and failure.get('reason') in {'metal-device-unavailable','metal-simd-unavailable'}
        and failure.get('requested_backend')=='metal' and failure.get('offloaded_layers')==0)

def run_inference(command, payload, timeout=180):
    # /usr/bin/time is a wrapper: kill its whole group on timeout, including
    # the native model process, before measuring the next case.
    process=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE,text=True,start_new_session=True)
    try:
        stdout,stderr=process.communicate(payload,timeout=timeout)
    except subprocess.TimeoutExpired:
        try:os.killpg(process.pid,signal.SIGKILL)
        except ProcessLookupError:pass
        process.communicate()
        raise
    return subprocess.CompletedProcess(command,process.returncode,stdout,stderr)

def infer(binary,model,backend,raw,context):
    command=[str(binary.resolve()),str(model.resolve()),backend]
    if platform.system()=='Darwin':command=['/usr/bin/time','-l',*command]
    elif platform.system()=='Linux':command=['/usr/bin/time','-f','%M maximum resident set size',*command]
    started=time.monotonic()
    result=run_inference(command,json.dumps(dict(transcript=raw,context=context),ensure_ascii=False))
    memory=re.search(r'(\d+)\s+maximum resident set size',result.stderr)
    rss=int(memory[1])*(1024 if platform.system()=='Linux' else 1) if memory else None
    item=dict(wall_ms=(time.monotonic()-started)*1000,peak_rss_bytes=rss,rejected=False,failed=result.returncode!=0)
    if result.returncode:
        # A rejected preview preserves raw ASR in the actual app. Include both
        # rejection count and fallback text; never present fallback as a correction.
        try:failure=json.loads(result.stdout)
        except (ValueError,TypeError):failure={"error":"runtime","stage":"unknown"}
        # This evaluator accepts only the fixed public corpus/synthetic cases.
        # Keep fatal signal diagnostics here; the product discards native stderr.
        item.update(text=raw,model_report=None,rejected=failure.get('error')=='rejected',failure=failure,
                    exit_status=result.returncode,diagnostic_stderr=result.stderr)
    else:
        report=json.loads(result.stdout)
        if report['backend']!=backend or (backend=='metal' and raw and report['offloaded_layers']<1) or (backend=='cpu' and report['offloaded_layers']!=0):
            raise ValueError('Requested correction backend was not actually used')
        item.update(text=report['text'],model_report=report)
    return item

def load_asr(directory, corpus, source_revision, require_network_denied, asr_model="small"):
    metadata=json.loads((directory/'comparison.json').read_text())
    if (metadata.get('protocol_version')!='context-evaluation-v2'
        or metadata.get('model')!=asr_model or metadata.get('backend')!='cpu'
        or metadata.get('model_sha256')!=ASR_MODELS[asr_model]['sha256']
        or metadata.get('corpus_sha256')!=checksum(ROOT/'evaluation/corpus.json')
        or metadata.get('source_revision')!=source_revision
        or not re.fullmatch('[0-9a-f]{64}',str(metadata.get('binary_sha256','')))
        or (require_network_denied and not metadata.get('network_denied'))):
        raise ValueError('ASR provenance mismatch; regenerate context reports for this revision')
    expected=[case['id'] for case in corpus['cases']]
    reports={}
    for mode in ['none','related']:
        path=directory/(mode+'.json')
        if checksum(path)!=metadata.get('asr_sha256',{}).get(mode):
            raise ValueError('ASR report checksum mismatch')
        report=json.loads(path.read_text())
        if (report.get('model_name')!=ASR_MODELS[asr_model].get('engine_name',asr_model) or report.get('backend')!='cpu'
            or [case['id'] for case in report['cases']]!=expected):
            raise ValueError('Incomplete or wrong ASR evaluation input')
        for case in report['cases']:
            if len(case['observations'])!=2 or any(not isinstance(row.get('text'),str) for row in case['observations']):
                raise ValueError('Incomplete ASR observations')
        reports[mode]=report
    return reports,metadata


def route_summary(records):
    result={}
    for mode in MODES:
        rows=[r for r in records if r['mode']==mode]
        spoken=[r for r in rows if r['reference']]
        chars=sum(r['final_score']['reference_chars'] for r in spoken)
        memory=[r['peak_rss_bytes'] for r in rows if r['peak_rss_bytes'] is not None]
        result[mode]=dict(completed=len(rows),
            raw_cer=sum(r['raw_score']['edits'] for r in spoken)/chars if chars else None,
            cer=sum(r['final_score']['edits'] for r in spoken)/chars if chars else None,
            rejected=sum(r['rejected'] for r in rows),
            runtime_failures=sum(r['failed'] and not r['rejected'] for r in rows),
            mean_wall_ms=sum(r['wall_ms'] for r in spoken)/len(spoken) if spoken else None,
            peak_rss_bytes=max(memory) if memory else None,
            silence_false_positives=sum(bool(r['text']) for r in rows if not r['reference']))
    return result


def render_summary(report):
    def percent(value):return f'{value:.2%}' if value is not None else 'pending'
    lines=[f"Complete: {report['complete']}",'',
           '| Context routing | Completed | Raw ASR CER | Final CER | Rejected | Runtime failures | Mean spoken ms | Peak MiB |',
           '|---|---:|---:|---:|---:|---:|---:|---:|']
    for mode,item in report['summary'].items():
        ms=f"{item['mean_wall_ms']:.0f}" if item['mean_wall_ms'] is not None else 'pending'
        rss=f"{item['peak_rss_bytes']/1048576:.1f}" if item['peak_rss_bytes'] is not None else 'unknown'
        lines.append(f"| {mode} | {item['completed']} | {percent(item['raw_cer'])} | {percent(item['cer'])} | {item['rejected']} | {item['runtime_failures']} | {ms} | {rss} |")
    if report['metal_skipped']:metal='Metal NOT TESTED: explicit CPU-only evaluation.'
    elif report['metal_smoke'] is None:metal='Metal not tested yet.'
    elif report['metal_unavailable']:metal='Metal UNAVAILABLE: required hardware capability is absent; inference NOT verified.'
    elif report['metal_smoke']['failed']:metal='Metal FAILED: inference NOT verified.'
    else:metal=f"Verified Metal inference: {report['metal_smoke']['model_report']['offloaded_layers']} layers offloaded."
    lines+=['',metal,'',report['limitation'],'','Supplemental generated outputs (raw fallbacks excluded):',
            json.dumps(report['quality'],ensure_ascii=False),'',
            '| Case | Rejected | Runtime failure | Facts pass | Format pass |',
            '|---|---|---|---|---|']
    for item in report['supplemental']:
        # A fallback's constraints remain available in JSON, but are never
        # displayed as a successful generated candidate.
        facts=str(item['constraints']['facts_ok']) if not item['failed'] else 'fallback'
        formatting=str(item['constraints']['format_ok']) if not item['failed'] else 'fallback'
        lines.append(f"| {item['case']['id']} | {item['rejected']} | {item['failed'] and not item['rejected']} | {facts} | {formatting} |")
    if report.get('baseline'):
        lines+=['','Frozen baseline: same original cases, re-scored with current checks.',
                'Before: '+json.dumps(report['baseline']['before']),
                'After: '+json.dumps(report['baseline']['after']),report['baseline']['limitation']]
    return '\n'.join(lines)+'\n'


def persist_report(output, report):
    report['summary']=route_summary(report['records'])
    report['quality']=quality_summary(report['supplemental'])
    files={'report.json':json.dumps(report,ensure_ascii=False,indent=2)+'\n',
           'summary.md':render_summary(report)}
    diffs=[]
    for item in report['records']:
        diffs.append(f"# {item['id']} / {item['mode']}: rejected={item['rejected']}, failed={item['failed']}\n")
        diffs.append(diff_text(item['raw_asr'],item['text'],item['id']+'/'+item['mode']))
    for item in report['supplemental']:
        diffs.append(f"# {item['case']['id']}: rejected={item['rejected']}, failed={item['failed']}\n")
        diffs.append(diff_text(item['case']['transcript'],item['text'],'fixed/'+item['case']['id']))
    files['cases.diff']='\n'.join(diffs)
    if report.get('baseline'):
        files['baseline.diff']='\n'.join(pair['diff'] for pair in report['baseline']['pairs'])
    for name,data in files.items():
        temporary=output/(name+'.tmp')
        temporary.write_text(data)
        temporary.replace(output/name)


def measured_infer(binary,model,backend,raw,context):
    started=time.monotonic()
    try:return infer(binary,model,backend,raw,context)
    except subprocess.TimeoutExpired:
        return dict(wall_ms=(time.monotonic()-started)*1000,peak_rss_bytes=None,
                    rejected=False,failed=True,text=raw,model_report=None,
                    failure=dict(error='runtime',stage='subprocess-timeout'))


def route_requests(corpus,asr):
    for index,case in enumerate(corpus['cases']):
        for mode,(asr_context,correction_context) in MODES.items():
            yield dict(id=case['id'],mode=mode,
                       raw_asr=asr['related' if asr_context else 'none']['cases'][index]['observations'][0]['text'],
                       reference=case['reference'],context=case['hint'] if correction_context else '')


def host_info():
    memory=(int(subprocess.check_output(['/usr/sbin/sysctl','-n','hw.memsize'],text=True))
            if platform.system()=='Darwin' else os.sysconf('SC_PHYS_PAGES')*os.sysconf('SC_PAGE_SIZE'))
    return dict(system=platform.system(),machine=platform.machine(),logical_cpus=os.cpu_count(),physical_memory_bytes=memory)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--model',type=Path,required=True)
    parser.add_argument('--model-name',choices=MODELS,required=True)
    parser.add_argument('--asr',type=Path,required=True)
    parser.add_argument('--asr-model',choices=ASR_MODELS,default='small')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--baseline',type=Path)
    parser.add_argument('--cpu-only',action='store_true')
    parser.add_argument('--require-network-denied',action='store_true')
    args=parser.parse_args()
    if args.require_network_denied:
        try:socket.create_connection(('127.0.0.1',9),timeout=1)
        except PermissionError:pass
        else:raise ValueError('Network sandbox was not enforced')
    if checksum(args.model)!=MODELS[args.model_name]['sha256']:raise ValueError('Correction model checksum mismatch')
    corpus=json.loads((ROOT/'evaluation/corpus.json').read_text())
    supplemental_cases=load_cases(ROOT/'evaluation/correction-cases.json')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    asr,asr_metadata=load_asr(args.asr,corpus,revision,args.require_network_denied,args.asr_model)
    baseline=load_baseline(args.baseline,args.model_name,MODELS[args.model_name],supplemental_cases) if args.baseline else None
    args.output.mkdir(parents=True,exist_ok=True)
    report=dict(protocol_version='correction-quality-v2',complete=False,model=args.model_name,
        model_identity=MODELS[args.model_name],source_revision=revision,
        source_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip()),
        binary_sha256=checksum(args.binary),runtime_source_sha256=checksum(ROOT/'crates/local-correction/src/lib.rs'),
        cargo_lock_sha256=checksum(ROOT/'crates/local-correction/Cargo.lock'),
        network_denied=args.require_network_denied,backend='cpu',
        host=host_info(),
        asr_input_sha256={mode:checksum(args.asr/(mode+'.json')) for mode in asr},
        asr_binary_sha256=asr_metadata['binary_sha256'],
        asr_model=args.asr_model,asr_model_sha256=ASR_MODELS[args.asr_model]['sha256'],
        corpus_sha256=checksum(ROOT/'evaluation/corpus.json'),supplemental_sha256=checksum(ROOT/'evaluation/correction-cases.json'),
        expected_records=len(corpus['cases'])*len(MODES),expected_supplemental=len(supplemental_cases),
        records=[],supplemental=[],metal_smoke=None,metal_skipped=args.cpu_only,metal_unavailable=False,
        baseline=None,baseline_sha256=checksum(args.baseline) if args.baseline else None,
        limitation='Public read speech and fixed reference-vocabulary contexts, not live OCR or natural dictation accuracy. '
                   'Fixed-case fact/format checks do not establish general semantic equivalence. '
                   'Rejected previews retain raw ASR and are not generated successes. Timings include model loading; VM timings do not predict physical Mac performance.')
    persist_report(args.output,report)
    if not args.cpu_only:
        gpu=measured_infer(args.binary,args.model,'metal','会議は金曜日です','会議、金曜日。')
        report.update(metal_smoke=gpu,metal_unavailable=gpu['failed'] and metal_unavailable(gpu.get('failure',{})))
        (args.output/'metal-smoke.json').write_text(json.dumps(gpu,ensure_ascii=False,indent=2)+'\n')
        persist_report(args.output,report)
        if gpu['failed'] and not report['metal_unavailable']:raise ValueError('Actual Metal inference failed')
    for request in route_requests(corpus,asr):
        item=measured_infer(args.binary,args.model,'cpu',request['raw_asr'],request['context'])
        item.update(request,
                    raw_score=score(normalize(request['reference']),normalize(request['raw_asr'])),
                    final_score=score(normalize(request['reference']),normalize(item['text'])))
        report['records'].append(item)
        persist_report(args.output,report)
        print(request['id'],request['mode'],'rejected' if item['rejected'] else ('runtime-failed' if item['failed'] else 'generated'),flush=True)
    for case in supplemental_cases:
        item=measured_infer(args.binary,args.model,'cpu',case['transcript'],case['context'])
        item.update(case=case,constraints=constraints(item['text'],case))
        report['supplemental'].append(item)
        persist_report(args.output,report)
        print(case['id'],'rejected' if item['rejected'] else ('runtime-failed' if item['failed'] else 'generated'),flush=True)
    if baseline:report['baseline']=compare_baseline(baseline,report['supplemental'])
    report['complete']=True
    persist_report(args.output,report)
    print(render_summary(report))
    if any(item['failed'] and not item['rejected'] for item in report['records']+report['supplemental']):
        raise ValueError('Correction runtime failed; see report.json')
    if any(item['silence_false_positives'] for item in report['summary'].values()) or report['quality']['silence_false_positives']:
        raise ValueError('Silence invariant failed')


if __name__=='__main__':main()
