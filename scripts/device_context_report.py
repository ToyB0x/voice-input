"""Validate and merge explicitly shared on-device JSON; never infer missing pages or bodies."""
from copy import deepcopy

PART_KEYS={'rows','rowOffset','partIndex','partCount','totalRows'}
def merge(parts):
    if not parts: raise ValueError('No report supplied')
    first=parts[0]
    if first.get('kind')!='device-context-lab' or first.get('schemaVersion')!=1: raise ValueError('Unsupported device report')
    if first.get('includesText') is not True: raise ValueError('Analysis requires an explicitly shared report including text/context')
    if first.get('completed') is not True or first.get('phase')!='done': raise ValueError('Experiment is incomplete or cancelled')
    if not isinstance(first.get('reportId'),str) or not first['reportId'] or len(first['reportId'])>128: raise ValueError('Invalid run identity')
    total=first.get('totalRows')
    if type(total) is not int or not 0<total<=2048: raise ValueError('Invalid row count')
    metadata={k:v for k,v in first.items() if k not in PART_KEYS}
    merged=[None]*total
    for report in parts:
        if {k:v for k,v in report.items() if k not in PART_KEYS}!=metadata: raise ValueError('Do not mix runs or privacy selections')
        offset,rows=report.get('rowOffset'),report.get('rows')
        if type(offset) is not int or not isinstance(rows,list) or offset<0 or offset+len(rows)>total: raise ValueError('Invalid part range')
        for i,row in enumerate(rows,offset):
            if merged[i] is not None: raise ValueError('Duplicate or overlapping report parts')
            if not isinstance(row,dict): raise ValueError('Invalid row')
            merged[i]=row
    if any(x is None for x in merged): raise ValueError('Report parts are missing; copy the remaining pages')
    result=deepcopy(first);result['rows']=merged;result['rowOffset']=0;result['partIndex']=None
    return result
