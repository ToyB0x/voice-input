#!/usr/bin/env python3
"""Six light LT slides, using only completed real measurements."""
import argparse,base64,html,json,os
from pathlib import Path
from ame_lt import ROOT
def e(v):return html.escape(str(v))
def data(path,mime):return 'data:'+mime+';base64,'+base64.b64encode(path.read_bytes()).decode()
def main():
    p=argparse.ArgumentParser();p.add_argument('--report',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    r=json.loads(a.report.read_text())
    if r.get('kind')!='ame-lt-three-stages' or not r.get('runtime_pass') or not r.get('complete'):raise ValueError('Completed real measurement required')
    rows=r['rows'];c=r['config'];slides=[]
    def output(stage,key):
        values=list(dict.fromkeys(x['final'] for x in rows if x['stage']==stage and x['id']==key))
        if not values:raise ValueError('Missing measured condition')
        return '<br>'.join(e(x) for x in values)
    def add(kicker,title,body):
        slides.append(f'<section><div class="kicker">{kicker}</div><h1>{title}</h1>{body}<footer>同じ合成音声で遊ぶ、小さな実験 <span>{len(slides)+1} / 6</span></footer></section>')
    play='<button class="play" data-play>▶ 同じ声を再生</button>'
    add('声に、ヒントを足してみる','この「あめ」、どっち？',
        '<div class="duel"><div class="rain">雨<span>ざあざあ</span></div><b>？</b><div class="candy">飴<span>わくわく</span></div></div>'
        +'<p class="utterance">「'+e(c['utterance'])+'」</p>'+play+'<p class="small">音声はHTML版で再生できます。</p>')
    add('Lv.1　耳だけ','まずは、音声入力。',
        '<div class="center"><p>追加の手がかり：なし</p><div class="answer rain">'+output('audio','no-context')+'</div>'+play+'</div>'
        +'<p class="bottom">この声に、あとから別の景色を足してみます。</p>')
    cards=[]
    for case in c['texts']:
        color='rain' if case['expected']=='雨' else 'candy'
        cards.append('<article class="'+color+'"><p class="hint">'+e(case['text'])+'</p><div class="arrow">↓</div><div class="answer">'+output('text',case['id'])+'</div></article>')
    add('Lv.2　ひと言ヒント','声はそのまま。文章を足す。','<div class="cards">'+''.join(cards)+'</div><p class="bottom">入力：同じ音声 ＋ 短い文章</p>')
    cards=[]
    for case in c['images']:
        color='rain' if case['expected']=='雨' else 'candy'
        cards.append('<article class="'+color+'"><img alt="実験に使用した文字なし画像" src="'+data(ROOT/case['file'],'image/png')+'"><div class="answer">'+output('image',case['id'])+'</div></article>')
    add('Lv.3　絵だけのヒント','今度は、絵を見せる。','<div class="cards images">'+''.join(cards)+'</div><p class="bottom">入力：同じ音声 ＋ 文字のない画像。人が付けた説明文は渡しません。</p>')
    add('仕組みは、小さく分ける','同じ声から、漢字を選び直す。',
        '<div class="flow"><div>音声<span>Whisper</span></div><b>→</b><div>「あめ」<span>音声認識結果</span></div><b>→</b><div class="accent">雨 ／ 飴 ／ 不明<span>文章・画像の手がかり</span></div></div>'
        '<p class="explain">文章と画像は、同じローカルモデルに渡します。</p>'
        '<p>モデルが選ぶのは「あめ」の表記。文の残りはコードで保持します。</p>')
    context_rows=[x for x in rows if x['stage']!='audio'];count=sum(x['passed'] for x in context_rows)
    run=os.environ.get('EVIDENCE_RUN_URL','ローカル実行')
    add('今日の、小さな実測','声は同じ。ヒントで変わる。',
        '<div class="result">'+str(count)+' / '+str(len(context_rows))+'<span>文章・画像の条件で、期待した全文に一致</span></div>'
        '<p>1つの合成音声を再利用。各条件2回：'+('出力は一致しました。' if r['stable'] else '出力に揺れがありました。')+'</p>'
        '<p>「雨／飴／不明」を選ぶLT用デモです。一般的な音声入力の精度を測った数字ではありません。</p>'
        '<p class="small">Whisper large-v3 → Qwen3-VL-8B-Instruct / CPU・推論中の通信禁止<br>音声のアクセント差を判定する試験ではありません。画像は合成素材です。<br>実測・全入力・出力：<a href="'+e(run)+'">'+e(run)+'</a></p>')
    css="""@page{size:338.667mm 190.5mm;margin:0}*{box-sizing:border-box}body{margin:0;background:#e8edf5;color:#14213a;font:25px 'Noto Sans JP','Noto Sans CJK JP',sans-serif}section{width:1280px;height:720px;margin:24px auto;padding:38px 64px 54px;background:#fffdf9;position:relative;overflow:hidden;page-break-after:always}.kicker{font-size:22px;color:#657187;letter-spacing:.12em}h1{font-size:48px;line-height:1.3;margin:12px 0 24px;letter-spacing:-.03em}p{line-height:1.6;margin:14px 0}footer{position:absolute;bottom:16px;left:64px;right:64px;font-size:14px;color:#69758a}footer span{float:right}.rain{color:#176cba}.candy{color:#c94c76}.duel{display:flex;justify-content:center;align-items:center;gap:100px;height:260px;font-size:150px;font-weight:800}.duel span{display:block;font-size:22px;font-weight:500;text-align:center}.duel b{font-size:64px;color:#aeb7c4}.utterance{text-align:center;font-size:40px}.play{display:block;margin:16px auto;padding:12px 24px;border:0;border-radius:24px;background:#172c4a;color:white;font:inherit;font-size:22px;cursor:pointer}.small{font-size:16px;line-height:1.6;color:#647087}.center{text-align:center;padding:45px 0}.answer{font-size:38px;font-weight:800;line-height:1.5}.center .answer{font-size:66px;margin:34px 0}.bottom{font-size:23px;margin-top:30px}.cards{display:flex;gap:28px}.cards article{width:50%;padding:28px;border-radius:24px;background:#eff6fd}.cards article.candy{background:#fff0f4}.hint{color:#22334c;font-size:28px;min-height:144px;margin:0}.arrow{font-size:36px;text-align:center;margin:12px}.images article{padding:16px}.images img{display:block;width:100%;height:300px;object-fit:contain;border-radius:12px}.images .answer{font-size:34px;margin:16px 0 6px;text-align:center}.flow{display:flex;align-items:center;justify-content:space-between;margin:65px 0 45px;gap:12px}.flow>div{padding:28px 22px;background:#eef3fa;border-radius:18px;font-size:34px;font-weight:700}.flow span{display:block;font-size:18px;font-weight:400;margin-top:14px}.flow .accent{background:#fff0d1}.explain{font-size:32px}.result{font-size:92px;font-weight:800;color:#176cba;margin:28px 0}.result span{font-size:24px;font-weight:400;display:inline-block;margin-left:28px;color:#24334b}a{color:inherit;overflow-wrap:anywhere}@media print{body{background:white}section{margin:0}.play{display:none}}"""
    audio=data(ROOT/c['audio'],'audio/wav')
    js="""const audio=document.querySelector('audio');document.querySelectorAll('[data-play]').forEach(b=>b.onclick=()=>{audio.currentTime=0;audio.play()});let page=0;document.addEventListener('keydown',e=>{if(['ArrowRight','PageDown','ArrowLeft','PageUp'].includes(e.key)){e.preventDefault();page=Math.max(0,Math.min(5,page+(['ArrowRight','PageDown'].includes(e.key)?1:-1)));document.querySelectorAll('section')[page].scrollIntoView({behavior:'smooth'})}});"""
    a.output.mkdir(parents=True,exist_ok=True)
    (a.output/'slides.html').write_text('<!doctype html><html lang="ja"><meta charset="utf-8"><title>この「あめ」、どっち？</title><style>'+css+'</style><body>'+''.join(slides)+'<audio src="'+audio+'"></audio><script>'+js+'</script></body></html>')
    (a.output/'results.json').write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
    notes=['# LTメモ（約3〜4分）','','1. 音声を再生。「どちらのあめを想像しましたか？」','2. 音声だけの実出力を見せる。正解・不正解は付けない。','3. 台風とハロウィンの短い文章を見せ、同じ声で結果を比較。','4. 次は画像だけ。人間が画像の説明を書いて渡していないことを説明。','5. 音声認識後、手がかりで漢字を選び直す仕組み。雨／飴／不明の候補はあらかじめ与える。','6. 実測結果をそのまま紹介。一般精度ではなく、同じ声の解釈が変わる小さなデモ。','','実験では2回のASR結果を全条件で共有。画像モデルが生の音声を聞く構成ではない。','文章ヒントは意図的に分かりやすくした。想定どおりにならない出力も隠さない。']
    (a.output/'speaker-notes.md').write_text('\n'.join(notes)+'\n')
    (a.output/'summary.md').write_text(f'# この「あめ」、どっち？\n\n文章・画像条件の全文一致: {count}/{len(context_rows)}\n\nQuality gate: {r["quality_pass"]}\n\n'+r['limitation']+'\n')
if __name__=='__main__':main()
