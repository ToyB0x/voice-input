#!/usr/bin/env python3
"""Generate a standalone Japanese slide deck only from a completed measured report."""
import argparse
import base64
from collections import Counter
import html
import json
import os
from pathlib import Path
import statistics

ROOT=Path(__file__).resolve().parents[1]
LABELS={'none':'文脈なし','fixture-text':'テキスト直接入力（対照）','ax':'AX API','ocr':'文字画像→OCR','image-ocr':'文字なし画像→OCR','vision-labels':'画像→Vision分類','mismatched-text':'逆の文脈（負の対照）'}
def e(x): return html.escape(str(x))
def image(path): return 'data:image/png;base64,'+base64.b64encode(path.read_bytes()).decode()
def table(headers,rows):
    return '<table><thead><tr>'+''.join('<th>'+e(h)+'</th>' for h in headers)+'</tr></thead><tbody>'+''.join('<tr>'+''.join('<td>'+e(c)+'</td>' for c in row)+'</tr>' for row in rows)+'</tbody></table>'
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    r=json.loads(a.report.read_text());assert r.get('completed') is True, 'Incomplete experiment; do not publish measured slides'
    a.output.mkdir(parents=True,exist_ok=True)
    rows=r['rows'];slides=[]
    def add(title,body): slides.append('<section><h1>'+title+'</h1>'+body+'<footer>Voice Input / 合成音声による小規模実験 · '+str(len(slides)+1)+'</footer></section>')
    add('音声だけで、漢字は決まる？','<p class="lead">あめがすごいですね</p><p class="lead">雨 ☔　／　飴 🍬</p><p>音声入力＋ローカルな画面文脈で、同音語の表記を比較する。</p><p>「音声入力 → 整文」を提供する製品例：<a href="https://www.typeless.com/ja">Typeless</a>、<a href="https://wisprflow.ai/">Wispr Flow</a>。製品間の性能比較や市場成長率は本実験の対象外。</p>')
    add('日本語について、何を検証するか','<p>日本語には同音異義語と複数の表記がある。アクセントも手掛かりになるが、音声だけで意図が一意になるとは限らない。</p><p><b>仮説：</b>関連する画面文脈で、意図した表記を選びやすくなる。逆の文脈では、誤誘導も起こり得る。</p><p>「海外言語より日本語が弱い」は今回のデータでは検証していない。他言語の対照群が必要。</p>')
    add('同じ音声・同じモデルで条件を変える',table(['項目','実験条件'],[
        ['音声',r['fixture']['utterance']+' / '+r['fixture']['voice']+' / 合成音声1本'],
        ['音声認識',r['models']['speech']+' / CPU'],['最終補正',r['models']['correction']+' / CPU'],
        ['利用先','Whisperヒントのみ／最終補正のみ／両方'],['追加指示','標準／自然な漢字表記を希望'],
        ['反復','2回・2回目はシーンと条件の順序を反転'],['推論','直列実行・対象プロセスのネットワーク禁止']])+'<p class="small">同じ合成音声を雨／飴の2シーンへ再利用。反復は独立した話者・発話サンプルではない。</p>')
    captures=[]
    for scene,c in r['captures'].items():
        captures.append([scene,c['ax']['status'],c['ocr']['status'],len(c['ocr'].get('text','')),c['vision']['status'],c['imageOCR']['status'],len(c['imageOCR'].get('text',''))])
    add('取得を測る：AX・OCR・画像分類',table(['シーン','AX','文字OCR','文字数','画像分類','画像OCR','文字数'],captures)+'<p>AXは実際のconsumer APIを呼び、権限拒否などは未取得のまま記録。直接与えた対照テキストとは区別する。</p><p>PNGは専用AppKitウィンドウの描画結果。別アプリの撮影・画面収録権限の成功を証明するものではない。</p>')
    summary=[]
    for source in LABELS:
        for scene in ['storm','candy']:
            selected=[x for x in rows if x.get('source')==source and x.get('scene')==scene and x.get('engine')=='qwen3' and x.get('instruction')=='kanji' and x.get('route')==('none' if source=='none' else 'both')]
            count=Counter(x.get('judgment') for x in selected)
            summary.append([LABELS[source],scene,f"{count['target']}/{len(selected)}" if selected else '未実行',
                f"{count['opposite']}/{len(selected)}" if selected else '—',selected[0].get('finalText','') if selected else '取得不可'])
    # One page per scene keeps all source conditions legible.
    for scene,name in [('storm','台風 → 雨'),('candy','お菓子 → 飴')]:
        add('実測：'+name,table(['条件','期待語／実行数','逆の語／実行数','最初の最終出力'],[[x[0],*x[2:]] for x in summary if x[1]==scene])+'<p class="small">Qwen＋漢字指示、文脈ありは「ヒントと最終補正の両方」。未実行は0点と混同しない。期待語の出現判定であり、全文の意味保持を採点したものではない。</p>')
    pics=[]
    for scene in ['storm','candy']:
        c=r['captures'][scene]
        labels=c['vision'].get('labels',[])
        pics.append('<div><img src="'+image(ROOT/'evaluation/context-evidence/assets'/f'{scene}.png')+'"><p>'+e(', '.join(f"{x['label']} {x['confidence']:.2f}" for x in labels[:5]) or '分類未取得')+'</p></div>')
    add('文字のない画像は、OCRだけでは足りる？','<div class="pictures">'+''.join(pics)+'</div><p>画像 → Apple Vision分類ラベル → テキスト補正。正解語への手動マッピングは行っていない。</p><p>画像はAI生成の合成刺激。画像を直接LLMへ渡すVLMの試験や、画面全体を読む製品機能ではない。</p>')
    route_rows=[]
    for source in ['fixture-text','ocr','vision-labels','mismatched-text']:
        for route in ['hint','correction','both']:
            for instruction in ['default','kanji']:
                xs=[x for x in rows if x.get('source')==source and x.get('route')==route and x.get('instruction')==instruction and x.get('engine')=='qwen3']
                if xs: route_rows.append([LABELS[source],route,instruction,f"{sum(x['judgment']=='target' for x in xs)}/{len(xs)}",round(statistics.median(x['correctionMs'] for x in xs))])
    for offset in range(0,len(route_rows),8):
        add('利用先と追加指示を比較する',table(['取得元','利用先','指示','期待語／実行数','補正中央値ms'],route_rows[offset:offset+8])+'<p class="small">推論時間はCI CPU上の実測。Qwen重みは再利用、発話ごとにKVを初期化。ASRの同一入力は条件間で共有。Mac実機の待ち時間を示す値ではない。</p>')
    apple=r['foundation']
    add('確認できた範囲と、残る検証','<p>Apple Foundation Models / CI：<b>'+e(apple.get('availability',apple.get('failure','unknown')))+'</b>。利用不可なら推論結果は存在しない。</p><p>この実験は1発話・2シーンのパイロット。一般的な精度改善、他言語や他製品への優位性は結論にしない。</p><p>次の検証：実話者・複数の同音語・文脈が無関係／誤っている条件・直接画像を扱うローカルVLM・実機のAX／撮影。</p><p>数値・否定・名前の保持と、期待語の選択は別の評価軸。採用ガードの通過だけでは品質を保証しない。</p>')
    run=os.environ.get('EVIDENCE_RUN_URL','ローカル実行')
    add('再現性と出典','<p>CI：'+e(run)+'</p><p>ソース：<code>'+e(r['sourceRevision'])+'</code></p><p>音声・画像・モデル・実行ファイルのSHA-256、実入力、全候補・棄却・時間は同梱のresults.jsonに記録。</p><p>一次資料：<a href="https://www.typeless.com/ja">Typeless</a> / <a href="https://wisprflow.ai/">Wispr Flow</a> / <a href="https://developer.apple.com/documentation/vision/vnclassifyimagerequest">Apple Vision分類</a> / <a href="https://developer.apple.com/documentation/vision/vnrecognizetextrequest">Apple Vision OCR</a> / <a href="https://developer.apple.com/documentation/foundationmodels">Foundation Models</a></p>')
    css='''@page{size:338.667mm 190.5mm;margin:0}*{box-sizing:border-box}body{margin:0;color:#17273d;background:#eef2f7;font:23px "Noto Sans CJK JP",sans-serif}section{position:relative;width:1280px;height:720px;padding:42px 64px;background:white;page-break-after:always;overflow:hidden;margin:24px auto}h1{font-size:38px;margin:0 0 24px;color:#173f7a}p{line-height:1.65;margin:16px 0}.lead{font-size:64px;line-height:1.3}.small{font-size:17px}table{width:100%;border-collapse:collapse;font-size:19px}td,th{border-bottom:1px solid #d4dfec;padding:9px 10px;text-align:left}th{background:#eef4fb}footer{position:absolute;bottom:18px;left:64px;color:#617085;font-size:14px}.pictures{display:flex;gap:32px}.pictures>div{width:48%;font-size:17px}.pictures img{width:100%;height:285px;object-fit:contain}a{color:#2356a4}code{font-size:18px}@media print{body{background:white}section{margin:0}}'''
    (a.output/'slides.html').write_text('<!doctype html><html lang="ja"><meta charset="utf-8"><title>音声と画面文脈の実測</title><style>'+css+'</style><body>'+''.join(slides)+'''<script>document.fonts.ready.then(()=>{requestAnimationFrame(()=>{const clipped=[...document.querySelectorAll('section')].filter(s=>{const limit=s.querySelector('footer').getBoundingClientRect().top-8;return [...s.children].filter(c=>c.tagName!=='FOOTER').some(c=>c.getBoundingClientRect().bottom>limit)});document.documentElement.dataset.layout=clipped.length?'failed':'passed';document.documentElement.dataset.clippedSlides=clipped.map(s=>[...document.querySelectorAll('section')].indexOf(s)+1).join(',');})});</script></body></html>''')
    (a.output/'results.json').write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
    lines=['# 日本語同音語・画面文脈の実測','',f'Source: `{r["sourceRevision"]}`',f'CI: {run}','',f'Apple: {apple.get("availability", "unknown")}', '', '|条件|シーン|期待語／実行数|逆の語／実行数|最初の最終出力|','|---|---|---:|---:|---|']
    lines += ['|'+'|'.join(str(x).replace('|','／').replace('\n',' ') for x in line)+'|' for line in summary]
    lines += ['','## 制約','']+['- '+x for x in r['limits']]
    (a.output/'summary.md').write_text('\n'.join(lines)+'\n')
    print(f'{len(slides)} slides generated from completed real measurements')
if __name__=='__main__': main()
