#!/usr/bin/env python3
"""Bundle pinned CPU text/vision CLIs. Download source only, never model weights."""
import datetime,json,os,platform,shutil,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def main():
    if platform.system()!='Darwin':raise SystemExit('App packaging requires macOS')
    spec=json.loads((ROOT/'evaluation/local-vlm/model.json').read_text())
    cache=ROOT/'.cache/demo-runtime';source=cache/'source';build=cache/'build'
    source.mkdir(parents=True,exist_ok=True)
    if not (source/'.git').exists():
        for args in [['init','-q'],['remote','add','origin',spec['runtime']['repo']],['fetch','--depth','1','origin',spec['runtime']['revision']],['checkout','--detach','FETCH_HEAD']]:
            subprocess.run(['git','-C',str(source),*args],check=True)
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=spec['runtime']['revision']:raise ValueError('Wrong demo runtime source')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain','--untracked-files=no'],text=True).strip():raise ValueError('Modified runtime source')
    date=datetime.datetime.fromisoformat(subprocess.check_output(['git','-C',str(source),'show','-s','--format=%cI','HEAD'],text=True).strip())
    if datetime.datetime.now(datetime.timezone.utc)-date<datetime.timedelta(hours=48):raise ValueError('Runtime requires 48-hour cooling')
    minimum=json.loads((ROOT/'src-tauri/tauri.conf.json').read_text())['bundle']['macOS']['minimumSystemVersion']
    subprocess.run(['cmake','-S',str(source),'-B',str(build),'-DCMAKE_BUILD_TYPE=Release',f'-DCMAKE_OSX_DEPLOYMENT_TARGET={minimum}',f'-DCMAKE_OSX_ARCHITECTURES={platform.machine()}',
        '-DBUILD_SHARED_LIBS=OFF','-DGGML_OPENMP=OFF','-DGGML_NATIVE=OFF','-DGGML_CUDA=OFF','-DGGML_METAL=OFF','-DLLAMA_OPENSSL=OFF','-DLLAMA_BUILD_TESTS=OFF','-DLLAMA_BUILD_SERVER=OFF','-DMTMD_VIDEO=OFF'],check=True)
    subprocess.run(['cmake','--build',str(build),'--target','llama-mtmd-cli','llama-completion','-j','4'],check=True)
    dest=ROOT/'src-tauri/resources';dest.mkdir(exist_ok=True)
    for name in ['llama-mtmd-cli','llama-completion']:
        target=dest/name;shutil.copy2(build/'bin'/name,target)
        subprocess.run(['codesign','--force','--sign','-',str(target)],check=True)
        # Refuse non-system dylib dependencies that would break installed apps.
        links=subprocess.check_output(['otool','-L',str(target)],text=True).splitlines()[1:]
        if any(not line.strip().startswith(('/usr/lib/','/System/Library/')) for line in links):raise ValueError('Demo runtime has an unbundled library dependency')
    notices=f"llama.cpp {spec['runtime']['revision']}\n{spec['runtime']['repo']}\n\n"+(source/'LICENSE').read_text()
    for license in sorted((source/'licenses').glob('*')):
        if license.is_file():notices+='\n\n'+license.name+'\n'+license.read_text()
    (dest/'DEMO-NOTICES.txt').write_text(notices)
if __name__=='__main__':main()
