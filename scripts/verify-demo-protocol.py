#!/usr/bin/env python3
"""Compare actual app Rust prompts/parser against the measured LT experiment."""
import argparse,json,subprocess
from pathlib import Path
from ame_lt import config,prompt_for,SCHEMA,IMAGE_SCHEMA,apply_spelling,parse_runtime_choice
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--report',type=Path);a=p.parse_args()
rows=json.loads(a.report.read_text())['rows'] if a.report else [dict(stage=stage,raw='雨がすごいですね。',context=case.get('text'),model_output='{"spelling":"飴"} [end of text]' if stage=='text' else '{"observation":"屋根からお菓子が落ちる","spelling":"飴"}') for stage,key in [('text','texts'),('image','images')] for case in config()[key]]
count=0
for row in rows:
    stage=row['stage']
    if stage=='audio':continue
    raw=row['raw'];text=row.get('context') if stage=='text' else None
    payload=dict(raw=raw,text=text,output=row['model_output'])
    out=json.loads(subprocess.check_output([str(a.binary.resolve())],input=json.dumps(payload),text=True))
    assert out['prompt']==prompt_for(raw,stage,{'text':text})
    assert out['schema']==(SCHEMA if stage=='text' else IMAGE_SCHEMA)
    assert out['final']==apply_spelling(raw,parse_runtime_choice(row['model_output'],stage))
    count+=1
print(f'App Rust prompt/schema/parser matches LT Python: {count} rows')
