#!/usr/bin/env python3
"""Generate a standalone Japanese slide deck only from a completed measured report."""
import argparse
import base64
import difflib
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
def comparable(rows, scene, source, engine="qwen3", instruction="kanji", route="both"):
    return [x for x in rows if x.get("scene")==scene and x.get("source")==source and x.get("engine")==engine and x.get("instruction")==instruction and x.get("route")==("none" if source=="none" else route) and x.get("status") not in ["unavailable","asr-failed","error"] and x.get("finalText") is not None]

def condition_counts(rows, scene, source, engine="qwen3", instruction="kanji", route="both"):
    selected=comparable(rows,scene,source,engine,instruction,route)
    return dict(executed=len(selected),target=sum(x.get('judgment')=='target' for x in selected),opposite=sum(x.get('judgment')=='opposite' for x in selected))

def omitted_fragments(controls, instruction):
    removed=[]
    for item in controls:
        if item['instruction']!=instruction or not item['result'].get('accepted'): continue
        original,candidate=item['input'],item['result'].get('candidate','')
        for tag,i,j,_,_ in difflib.SequenceMatcher(None,original,candidate,autojunk=False).get_opcodes():
            if tag=='delete' and original[i:j].strip(): removed.append(original[i:j])
    return list(dict.fromkeys(removed))

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--engine',choices=['qwen3','apple-foundation']);p.add_argument('--input-form',choices=['original','reading'],default='original');a=p.parse_args()
    r=json.loads(a.report.read_text());assert r.get('completed') is True, 'Incomplete experiment; do not publish measured slides'
    a.output.mkdir(parents=True,exist_ok=True)
    engine=a.engine or r.get('presentationEngine','qwen3')
    text_only=r.get('inputKind')=='text'
    instruction_key=r.get('presentationInstruction','kanji')
    route=r.get('presentationRoute','correction' if r.get('inputKind')=='text' else 'both')
    engine_label='Apple Foundation Models' if engine=='apple-foundation' else r['models'].get('qwenName',r['models']['correction'] if 'Qwen' in r['models']['correction'] else 'Qwen（SHA-256はJSON）')
    footer_label={'recorded':'録音','text':'かな原文','bundled':'同梱音声'}.get(r.get('inputKind'),'合成音声')
    rows=[x for x in r['rows'] if x.get('inputForm','original')==a.input_form or x.get('engine')=='none'];slides=[]
    falling=r.get('experiment',{}).get('dataset')=='ame-falling-v1'
    scenes=['rainfall','candy-shower'] if falling else ['storm','candy']
    scene_names=['降雨 → 雨','屋根からの飴まき → 飴'] if falling else ['台風 → 雨','お菓子 → 飴']
    repeats=len({x['repeat'] for x in rows if 'repeat' in x})
    def add(title,body): slides.append('<section><h1>'+title+'</h1>'+body+'<footer>Voice Input / '+footer_label+'による小規模実験 · '+str(len(slides)+1)+'</footer></section>')
    add('音声だけで、漢字は決まる？','<p class="lead">'+e(r['fixture']['utterance'])+'</p><p class="lead">雨 ☔　／　飴 🍬</p><p>音声入力＋ローカルな画面文脈で、同音語の表記を比較する。</p><p>「音声入力 → 整文」を提供する製品例：<a href="https://www.typeless.com/ja">Typeless</a>、<a href="https://wisprflow.ai/">Wispr Flow</a>。製品間の性能比較や市場成長率は本実験の対象外。</p>')
    add('日本語について、何を検証するか','<p>日本語には同音異義語と複数の表記がある。アクセントも手掛かりになるが、音声だけで意図が一意になるとは限らない。</p><p><b>仮説：</b>関連する画面文脈で、意図した表記を選びやすくなる。逆の文脈では、誤誘導も起こり得る。</p><p>「海外言語より日本語が弱い」は今回のデータでは検証していない。他言語の対照群が必要。</p>')
    add('同じ原文・同じモデルで条件を変える' if text_only else '同じ音声・同じモデルで条件を変える',table(['項目','実験条件'],[
        ['入力',r['fixture']['utterance']+' / '+r['fixture']['voice']],
        ['音声認識',r['models']['speech']+' / '+r['models'].get('speechBackend','cpu')],['最終補正',engine_label+' / '+('OS管理' if engine=='apple-foundation' else r['models'].get('qwenBackend','cpu'))],
        ['利用先','最終補正のみ（音声・Whisperなし）' if text_only else 'Whisperヒントのみ／最終補正のみ／両方'],['入力方式',a.input_form+' / AFM '+r['models'].get('foundationProfile','従来')],['追加指示',' / '.join(k+': '+('空欄' if not v else 'あり') for k,v in r['instructions'].items())],
        ['反復',f'{repeats}回・偶数回目はシーンと条件の順序を反転'],['推論','直列実行 / '+('対象プロセスの通信禁止' if r.get('networkDenied') else 'ローカルモデルAPI（通信Sandboxなし）')]])+'<p class="small">同じ入力を雨／飴の2シーンへ再利用。反復は独立した話者・発話サンプルではない。</p>')
    if text_only:
        add('何を測っていて、何は測っていないか','<p class="lead">かな原文 → 文脈付き補正</p><p>入力は共有JSONのかな原文。音声認識を外し、文脈と追加指示による表記選択を切り分ける。</p><p>Whisperの認識精度やヒントの効果は未測定。先行CIのQwen＋合成音声とは入力・モデル・端末が異なるため、モデルの優劣を直接比較しない。</p>')
    captures=[]
    for scene,c in r['captures'].items():
        captures.append([scene,c['ax']['status'],c['ocr']['status'],len(c['ocr'].get('text','')),c['vision']['status'],c['imageOCR']['status'],len(c['imageOCR'].get('text',''))])
    add('取得を測る：AX・OCR・画像分類',table(['シーン','AX','文字OCR','文字数','画像分類','画像OCR','文字数'],captures)+'<p>AXは実際のconsumer APIを呼び、権限拒否などは未取得のまま記録。直接与えた対照テキストとは区別する。</p><p>PNGは専用AppKitウィンドウの描画結果。別アプリの撮影・画面収録権限の成功を証明するものではない。</p>')
    summary=[]
    for source in LABELS:
        for scene in scenes:
            selected=comparable(rows,scene,source,engine,instruction_key,route)
            count=Counter(x.get('judgment') for x in selected)
            summary.append([LABELS[source],scene,f"{count['target']}/{len(selected)}" if selected else '未実行',
                f"{count['opposite']}/{len(selected)}" if selected else '—',selected[0].get('finalText','') if selected else '取得不可'])
    # One page per scene keeps all source conditions legible.
    for scene,name in zip(scenes,scene_names):
        add('実測：'+name,table(['条件','期待語／実行数','逆の語／実行数','最初の最終出力'],[[x[0],*x[2:]] for x in summary if x[1]==scene])+'<p class="small">選択エンジン・追加指示での最終出力。利用先とモデル設定は実験条件とJSONを参照。未実行は0点と混同しない。期待語の出現判定であり、全文の意味保持を採点したものではない。</p>')
    if falling:
        full=[]
        for source in LABELS:
            for scene in scenes:
                xs=comparable(rows,scene,source,engine,instruction_key,route)
                full.append([LABELS[source],scene,f"{sum(x.get('finalPass') is True for x in xs)}/{len(xs)}" if xs else '未実行'])
        for offset in range(0,len(full),7):
            add('最終試験：語尾を含む文全体の一致',table(['取得元','シーン','期待文との一致／実行数'],full[offset:offset+7])+'<p>期待文は「雨が降っています／飴が降っています」。句読点と空白だけを除外。文字対照・画像分類・逆の文脈は別集計。</p>')
    pics=[]
    for scene in scenes:
        c=r['captures'][scene]
        labels=c['vision'].get('labels',[])
        pics.append('<div><img src="'+image(ROOT/'evaluation/context-evidence/assets'/f'{scene}.png')+'"><p>'+e(', '.join(f"{x['label']} {x['confidence']:.2f}" for x in labels[:5]) or '分類未取得')+'</p></div>')
    add('文字のない画像は、OCRだけでは足りる？','<div class="pictures">'+''.join(pics)+'</div><p>画像 → Apple Vision分類ラベル → テキスト補正。正解語への手動マッピングは行っていない。</p><p>画像はAI生成の合成刺激。画像を直接LLMへ渡すVLMの試験や、画面全体を読む製品機能ではない。</p>')
    route_rows=[]
    for source in ['fixture-text','ocr','vision-labels','mismatched-text']:
        for route_name in ['hint','correction','both']:
            for instruction in r['instructions']:
                xs=[x for x in rows if x.get('source')==source and x.get('route')==route_name and x.get('instruction')==instruction and x.get('engine')==engine and x.get('status') not in ['unavailable','asr-failed','error'] and x.get('finalText') is not None]
                if xs: route_rows.append([LABELS[source],route_name,instruction,f"{sum(x['judgment']=='target' for x in xs)}/{len(xs)}",round(statistics.median(x['correctionMs'] for x in xs if x.get('correctionMs') is not None)) if any(x.get('correctionMs') is not None for x in xs) else '未計測'])
    for offset in range(0,len(route_rows),8):
        add('利用先と追加指示を比較する',table(['取得元','利用先','指示','期待語／実行数','補正中央値ms'],route_rows[offset:offset+8])+'<p class="small">時間はこのJSONの実行環境での実測。Qwen重みの再利用は実行方式に依存し、AFM内部のロード時間はOS管理。ASRの同一入力は条件間で共有。</p>')
    # Show observed failures as first-class findings, not just aggregate scores.
    actual=[x for x in rows if x.get('engine')=='none' and x.get('source')=='none']
    raw_examples=list(dict.fromkeys(x['raw'] for x in actual if isinstance(x.get('raw'),str)))
    def counts(scene,source,kind='target'):
        value=condition_counts(rows,scene,source,engine,instruction_key,route)
        return f"{value[kind]}/{value['executed']}" if value['executed'] else '未実行'
    kana=[x for x in r.get('kanaControls',[]) if x.get('engine','qwen3')==engine]
    kana_candidates=[x for x in kana if isinstance(x['result'].get('candidate'),str)]
    kana_excerpts=[x for x in kana if isinstance(x['result'].get('candidateExcerpt'),str)]
    kana_changed=sum(x['result']['candidate'].rstrip('。')!=x['input'].rstrip('。') for x in kana_candidates)
    add('今回の実測で分かったこと',
        '<p>'+('入力したかな原文：' if text_only else '文脈なしのWhisper原文：')+'<b>'+e(' / '.join(raw_examples) or '未取得／失敗（原文はJSONを参照）')+'</b>。</p>'
        '<p>直接テキスト＋'+('最終補正' if text_only else 'ヒント・補正')+'の期待語出現：台風 '+counts('storm','fixture-text')+'、キャンディ '+counts('candy','fixture-text')+'。</p>'
        '<p>画像分類ラベル＋'+('最終補正' if text_only else 'ヒント・補正')+'の期待語出現：台風 '+counts('storm','vision-labels')+'、キャンディ '+counts('candy','vision-labels')+'。逆の文脈での逆語出現：台風 '+counts('storm','mismatched-text','opposite')+'、キャンディ '+counts('candy','mismatched-text','opposite')+'。</p>'
        '<p>かな入力の分離試験：候補本文 '+str(len(kana_candidates))+'件、長い候補の抜粋 '+str(len(kana_excerpts))+'件／全'+str(len(kana))+'件。完全な候補のうち'+str(kana_changed)+'件で表記が変化（末尾の句点を除く）。変化だけでは正しい変換や意味保持を保証しない。</p>')
    # Authored interpretation stays with its frozen evidence, not future reports.
    for note in r.get('presentationSlides',[]):
        body=table(note['headers'],note['rows']) if note.get('headers') else ''
        body+=''.join('<p>'+e(paragraph)+'</p>' for paragraph in note.get('paragraphs',[]))
        add(e(note['title']),body)
    controls=[x for x in r.get('preservationControls',[]) if x.get('engine','qwen3')==engine]
    for instruction in r['instructions']:
        label='標準指示' if instruction=='default' else '追加指示 '+instruction
        xs=[x for x in controls if x['instruction']==instruction]
        if xs:
            add('意味の保持も別に確認する：'+label,
                table(['原文','実際の補正候補','ガード判定'],[[x['input'],x['result'].get('candidate') or '未生成', '採用' if x['result'].get('accepted') is True else '棄却・原文利用' if x['result'].get('accepted') is False else '未実行／失敗・原文利用'] for x in xs])+
                '<p class="small">採用候補の文字差分で削除された部分：'+e('、'.join('「'+x+'」' for x in omitted_fragments(controls,instruction)) or 'なし')+'。削除の有無と意味保持は別の評価軸。候補・採否・最終出力はJSONで確認できる。</p>')
    apple=r['foundation']
    add('確認できた範囲と、残る検証','<p>Apple Foundation Models / この実行：<b>'+e(apple.get('availability',apple.get('failure','unknown')))+'</b>。利用不可なら推論結果は存在しない。</p><p>主試験は1入力・2シーンのパイロット。一般的な精度改善、他言語や他製品への優位性は結論にしない。</p><p>次の検証：実話者・複数の同音語・文脈が無関係／誤っている条件・直接画像を扱うローカルVLM・実機のAX／撮影。</p><p>数値・否定・名前の保持と、期待語の選択は別の評価軸。採用ガードの通過だけでは品質を保証しない。</p>')
    run=os.environ.get('EVIDENCE_RUN_URL','ローカル実行')
    add('再現性と出典','<p>測定元：'+e(run)+'</p><p>ソース：<code>'+e(r.get('sourceRevision','unknown'))+'</code></p><p>'+('利用者の共有結果から作成した解析用スナップショット。長い棄却候補は先頭抜粋のみ。元の全文JSONや省略部分を再構成したものではない。' if r.get('analysisSnapshot') else '音声・画像・モデル・実行ファイルのSHA-256、実入力、全候補・棄却・時間は同梱のresults.jsonに記録。')+'</p><p>一次資料：<a href="https://www.typeless.com/ja">Typeless</a> / <a href="https://wisprflow.ai/">Wispr Flow</a> / <a href="https://developer.apple.com/documentation/vision/vnclassifyimagerequest">Apple Vision分類</a> / <a href="https://developer.apple.com/documentation/vision/vnrecognizetextrequest">Apple Vision OCR</a> / <a href="https://developer.apple.com/documentation/foundationmodels">Foundation Models</a></p>')
    css='''@page{size:338.667mm 190.5mm;margin:0}*{box-sizing:border-box}body{margin:0;color:#17273d;background:#eef2f7;font:23px "Noto Sans JP","Noto Sans CJK JP",sans-serif}section{position:relative;width:1280px;height:720px;padding:42px 64px;background:white;page-break-after:always;overflow:hidden;margin:24px auto}h1{font-size:38px;margin:0 0 24px;color:#173f7a}p{line-height:1.65;margin:16px 0}.lead{font-size:64px;line-height:1.3}.small{font-size:17px}table{width:100%;border-collapse:collapse;font-size:19px}td,th{border-bottom:1px solid #d4dfec;padding:9px 10px;text-align:left}th{background:#eef4fb}footer{position:absolute;bottom:18px;left:64px;color:#617085;font-size:14px}.pictures{display:flex;gap:32px}.pictures>div{width:48%;font-size:17px}.pictures img{width:100%;height:285px;object-fit:contain}a{color:#2356a4}code{font-size:18px}@media print{body{background:white}section{margin:0}}'''
    (a.output/'slides.html').write_text('<!doctype html><html lang="ja"><meta charset="utf-8"><title>音声と画面文脈の実測</title><style>'+css+'</style><body>'+''.join(slides)+'''<script>document.fonts.ready.then(()=>{requestAnimationFrame(()=>{const clipped=[...document.querySelectorAll('section')].filter(s=>{const limit=s.querySelector('footer').getBoundingClientRect().top-8;return [...s.children].filter(c=>c.tagName!=='FOOTER').some(c=>c.getBoundingClientRect().bottom>limit)});document.documentElement.dataset.layout=clipped.length?'failed':'passed';document.documentElement.dataset.clippedSlides=clipped.map(s=>[...document.querySelectorAll('section')].indexOf(s)+1).join(',');})});</script></body></html>''')
    (a.output/'results.json').write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
    lines=['# 日本語同音語・画面文脈の実測','',f'Source: `{r["sourceRevision"]}`',f'CI: {run}','',f'Apple: {apple.get("availability", "unknown")}', '', '|条件|シーン|期待語／実行数|逆の語／実行数|最初の最終出力|','|---|---|---:|---:|---|']
    lines += ['|'+'|'.join(str(x).replace('|','／').replace('\n',' ') for x in line)+'|' for line in summary]
    lines += ['','## 制約','']+['- '+x for x in r['limits']]
    (a.output/'summary.md').write_text('\n'.join(lines)+'\n')
    print(f'{len(slides)} slides generated from completed real measurements')
if __name__=='__main__': main()
