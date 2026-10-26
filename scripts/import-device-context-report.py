#!/usr/bin/env python3
"""Merge JSON pasted from the device lab for analysis and slide generation."""
import argparse,json
from pathlib import Path
from device_context_report import merge
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('reports',type=Path,nargs='+')
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
def invalid_constant(value): raise ValueError('Non-finite JSON number: '+value)
reports=[]
for path in a.reports:
    if path.stat().st_size>1_500_000: raise SystemExit('A report part exceeds the copy limit')
    reports.append(json.loads(path.read_text(),parse_constant=invalid_constant))
result=merge(reports)
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(f"Merged {len(result['rows'])} measured rows from {result['reportId']}")
