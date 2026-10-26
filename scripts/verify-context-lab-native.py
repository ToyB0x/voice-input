#!/usr/bin/env python3
"""Probe the bundled controlled-window helper with real Vision; never claim unavailable OCR succeeded."""
import argparse,json,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',type=Path,required=True);p.add_argument('--assets',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
for scene in ['storm','candy']:
    # communicate supplies only the protocol acknowledgement; no user's screen or text.
    result=subprocess.run([str(a.binary.resolve()),str(a.assets.resolve()),scene],input=b'continue\n',capture_output=True,timeout=90)
    lines=result.stdout.splitlines();assert result.returncode==0 and len(lines)==2, 'Controlled capture helper failed'
    ready,capture=map(json.loads,lines)
    assert ready==dict(phase='ready',protocolVersion=1) and capture['scene']==scene
    for key in ['ocr','imageOCR','vision']: assert capture[key]['status'] in ['completed','unavailable']
    assert len(capture['fixtureText'])<512
    (a.output/f'{scene}.json').write_text(json.dumps(capture,ensure_ascii=False,indent=2)+'\n')
    print(scene,{k:capture[k]['status'] for k in ['ocr','imageOCR','vision']})
print('Controlled native helper completed; statuses above describe actual Vision availability, not assumed success.')
