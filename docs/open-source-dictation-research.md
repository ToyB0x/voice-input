# 公開音声入力アプリの調査とVoice Inputへの改善提案

調査日：2026-10-27（日本時間）。各リポジトリのコードを下記コミットで読み、README・作者報告・Issueと区別した。アプリの実推論や日本語ベンチマークをこちらで実行した調査ではない。参照コミットとファイルURLは[参照一覧](open-source-dictation-sources.json)に保存。

## 結論

公開実装で共通して参考になるのは、ASR・整形・文脈の役割分離、短い語彙ヒント、文脈の出所の分離、失敗時の原文保持。日本語の同音異義語や人名欠落をすべて解決する実装・実測は今回確認できなかった。全文かな化は私たちの実験では有望だが、これらのアプリの確立した標準手法として紹介できる根拠はない。

私たちには、原文を保持し、読みは単語候補選択の補助に使い、モデルが返す修正範囲をコードで制限する構成を推奨する。文字文脈の処理と画像理解は別の改善として測る。

## 1. 確認した実装

### VoiceInk

[補正指示](https://github.com/Beingpax/VoiceInk/blob/78aa19ddb5c8b2f378ac650184491c1f26c230f7/VoiceInk/Core/Enhancement/AIPrompts.swift#L11)は、意味・語句・文体・不確実性を保持し、不要な言い換えや未発話情報の追加を禁止する。[文脈構築](https://github.com/Beingpax/VoiceInk/blob/78aa19ddb5c8b2f378ac650184491c1f26c230f7/VoiceInk/Features/Enhancement/Workflows/AIEnhancementService.swift#L110)では独自辞書、選択テキスト、クリップボード、画面テキストを別ブロックとして渡す。辞書は明らかに該当する語の表記の根拠にし、無理な置換をしない。画面情報は本文の代わりではなく、綴りや参照先を明確にする参考としている。

[録音開始時の文脈取得](https://github.com/Beingpax/VoiceInk/blob/78aa19ddb5c8b2f378ac650184491c1f26c230f7/VoiceInk/Features/Recording/Context/RecordingContextSnapshot.swift#L36)で時点を揃える。[画面OCR](https://github.com/Beingpax/VoiceInk/blob/78aa19ddb5c8b2f378ac650184491c1f26c230f7/VoiceInk/Infrastructure/SystemIntegration/ScreenCapture/ScreenCaptureService.swift#L210)はアクティブなウィンドウを対象に、Vision accurate、言語補正オン、自動言語判定を使い、認識行を改行で結合する。つまりOCRの単語途中改行を特別に解決した例ではない。今回読んだ画面文脈経路はOCRテキスト化であり、画像を直接理解するVLM経路とは区別する。

[補正出力フィルタ](https://github.com/Beingpax/VoiceInk/blob/78aa19ddb5c8b2f378ac650184491c1f26c230f7/VoiceInk/Core/Enhancement/AIEnhancementOutputFilter.swift#L4)はthinking/reasoningタグ除去と前後空白の整理。この関数自体は、人名や数字の保存を検証する意味ガードではない。Ollama/local CLIへの経路もあるため、設計上の役割分離はローカル構成でも参考になる。設定したプロバイダによって通信先は異なる。

### Amical

[Whisperヒント構築](https://github.com/amicalhq/amical/blob/d5604ef548816d8729b1170696d315a35c00391a/apps/desktop/src/pipeline/providers/transcription/whisper-prompt.ts#L87)は語彙と直前の文章・カーソル前の文章を使い分ける。UTF-8全体800バイト、直前文脈60バイト、既定で直前10単語という実装上の上限を設ける。[正規化](https://github.com/amicalhq/amical/blob/d5604ef548816d8729b1170696d315a35c00391a/apps/desktop/src/pipeline/providers/transcription/whisper-prompt.ts#L55)は空白を圧縮し、UTF-8の境界を壊さず、Whisperが長いprefixの先頭を落とすことを意識して末尾を保持する。これらの数値はAmicalの設定であって、日本語に最適だと確認された値ではない。

[整形プロンプト](https://github.com/amicalhq/amical/blob/d5604ef548816d8729b1170696d315a35c00391a/apps/desktop/src/pipeline/providers/formatting/formatter-prompt.ts#L209)は原文・前後の入力欄・語彙を分け、チャット／メール／メモに応じて整形する。「すべての内容語を保持」「質問に答えない」「要約しない」を指示と例で示す。[幻覚除去](https://github.com/amicalhq/amical/blob/d5604ef548816d8729b1170696d315a35c00391a/apps/desktop/src/pipeline/utils/segment-filter.ts#L34)はnoSpeechProbと既知の幻覚フレーズを組み合わせる。単に辞書にある語が出たから消す処理ではない。

取り入れる点は、Whisperへ長い校正命令やOCR全文をそのまま渡すより、短い用語候補と局所的な文章を渡す設計。既存の768バイト制限を800へ変えるだけで改善する、という意味ではない。アプリ別の整形ルールも、単語訂正から切り離すと副作用を抑えやすい。

### Handy（Rust / Tauri）

[モデルの語彙機能への受け渡し](https://github.com/cjpais/Handy/blob/126bed73cb4c3c2afd0b23f3ae4e1ef03981f6c9/src-tauri/src/managers/transcription.rs#L1556)と、非対応時の後処理を区別する。語彙をすでにモデルへ渡した場合は曖昧一致の追加補正をスキップする。[曖昧一致](https://github.com/cjpais/Handy/blob/126bed73cb4c3c2afd0b23f3ae4e1ef03981f6c9/src-tauri/src/audio_toolkit/text.rs#L34)は編集距離とSoundexを使うが、このフォールバックはASCII語に限定し、CJKへ適用しないと明記されている。日本語にはそのまま転用できない点が重要。

[標準の整形指示](https://github.com/cjpais/Handy/blob/126bed73cb4c3c2afd0b23f3ae4e1ef03981f6c9/src-tauri/src/settings.rs#L840)は語順・意味の保持を要求する。[Apple Intelligenceを含む構造化出力経路](https://github.com/cjpais/Handy/blob/126bed73cb4c3c2afd0b23f3ae4e1ef03981f6c9/src-tauri/src/actions.rs#L250)もあり、AFMが利用できない、空の返答などは結果なしとして扱う。構造化出力の実装があっても、「田中さんへ」の保持をコードで保証した証拠とはならない。

### OpenWhispr

[通常のcleanup指示](https://github.com/OpenWhispr/openwhispr/blob/6408cd9a1081dc4935706fcbba6aaeae1d2cd18d/src/locales/en/prompts.json#L2)は、発話を命令として実行せず、語句・固有名詞を保持して整える。[二重生成ガード](https://github.com/OpenWhispr/openwhispr/blob/6408cd9a1081dc4935706fcbba6aaeae1d2cd18d/src/utils/cleanupOutput.ts#L19)はIntl.Segmenterで単語比較し、全文が二重に生成され、原文で実際に2回言われた根拠が乏しい場合に棄却する。[Whisperの辞書エコー判定](https://github.com/OpenWhispr/openwhispr/blob/6408cd9a1081dc4935706fcbba6aaeae1d2cd18d/src/utils/dictionaryEchoFilter.js#L17)もあり、辞書語の単なる重複とプロンプトの読み上げ・反復を区別しようとしている。

[スクリーンショットの送信先判定](https://github.com/OpenWhispr/openwhispr/blob/6408cd9a1081dc4935706fcbba6aaeae1d2cd18d/src/helpers/dictationRouting.js#L38)には、画像対応モデルや明示指定したvisionモデルへ画像を渡し、非対応なら画像を落とす経路がある。ただし[呼び出し元](https://github.com/OpenWhispr/openwhispr/blob/6408cd9a1081dc4935706fcbba6aaeae1d2cd18d/src/helpers/audioManager.js#L3399)では音声アシスタント要求時にスクリーンショットを消費している。通常のdictation cleanupが常に画像で同音語を修正する実装だとは言えない。またこの経路が自動的に完全ローカルだという意味でもない。

日本語に直接関係する[Issue #2490](https://github.com/OpenWhispr/openwhispr/issues/2490)は、固有名詞の表記だけでは読みを教えられず、辞書へspoken formを追加したいという提案。調査時点でOPENで、実装済み機能として扱わない。「表記＋読み」の組をASRと補正へ共有する案は私たちにも適している。

### Whispering / Epicenter

[現在のホーム画面実装](https://github.com/EpicenterHQ/epicenter/blob/7cc869b4a1295e2566b61e1a4d5967a89f413b9b/apps/whispering/src/routes/+page.svelte#L86)では、文字起こしを表示し、後処理は明示操作で実行、結果を原文と別ファイルにする。これは同音語を解決するアルゴリズムではなく、誤った書き換えから原文を守るUXの参考。私たちの場合、保存方式まで真似る必要はなく、メモリ上の原文と補正候補の分離を維持すればよい。

### 個人プロジェクト：VoiceInk向けQwen追加学習

hourliert/VoiceInk-Qwen3.5-2B-FTは一般アプリ本体と区別する。[文脈付きの学習入力構築](https://github.com/hourliert/VoiceInk-Qwen3.5-2B-FT/blob/95bbf17b228b7ee03aec02d82de2f03a30ae3694/src/training/prepare_dataset.py#L48)と評価スクリプトを公開している。作者の[反復増幅の報告](https://github.com/hourliert/VoiceInk-Qwen3.5-2B-FT/blob/95bbf17b228b7ee03aec02d82de2f03a30ae3694/docs/BLOG_POST.md#L250)では、長い反復的な入力で出力が増幅したため、長文・反復例をデータへ加えて追加学習した。[評価時の応答上限の問題](https://github.com/hourliert/VoiceInk-Qwen3.5-2B-FT/blob/95bbf17b228b7ee03aec02d82de2f03a30ae3694/docs/BLOG_POST.md#L175)も報告され、評価だけ短く打ち切ると製品で起きる反復を見逃すと述べている。

これは作者の環境・データ・評価者による報告で、日本語や私たちのMacでの優位性の実測ではない。RTX 4080 Superと外部LLMによるラベル付けを使う構成を、そのままローカル限定の私たちの運用へ持ち込むことはしない。学ぶ点は、モデルの大きさだけでなく失敗例を含む評価・学習データを整えること。

補足：[VoiceInk #893](https://github.com/Beingpax/VoiceInk/issues/893)には、Whisperの言語指定と英語の初期プロンプトが不一致になり、日本語でも翻訳・文字体系の揺れが起きた利用者報告がある。IssueはCLOSEDだが、報告だけから今回の全版に同じ問題がある／ないとは断定しない。私たちのヒント言語とASR言語の組を回帰試験にする根拠にはなる。

## 2. 私たちへの改善提案（ここからは独自提案）

### 最優先：原文からの限定置換と意味保持

AFMやQwenへ全文の書き直しを任せる代わりに、原文と読み付き辞書からアプリが置換範囲と候補を作り、モデルは候補IDと根拠文のIDだけを返す。引用は元の文脈からコードで復元する。アプリ側で候補の実在、重複範囲の不在、保護語や数字を壊していないことを確認してから適用する。同じ読みであることは呼び出し側の辞書に依存し、モデルや検証コードが独立に証明するものではない。かな全文を最終出力の原本にはせず、原文の曖昧な語を選び直す補助にする。

最初の許可操作は、根拠のある登録済み同音語の表記置換に絞る。現在の実装には品詞判定はない。名前・数値・宛先・否定・動作・語尾は保護し、フィラー除去と句読点・書式整形は別の段階／許可操作にする。保護語の抽出だけに頼れば未知の語を見逃すため、無関係な部分を書き換えない制約を主とし、ガードを補助にする。「田中さんへ→削除」「降っています→お祭りです」はこの経路では適用しない。

これは今回の公開アプリがそのまま採用していると確認した仕組みではなく、私たちの実測で明らかになった欠落・置き換えへの対策案。AFMもQwenも既に構造化出力の経路があるので、単にJSONにする提案ではなく、返す修正の単位とコードでの適用条件を変える。

### 次：OCR・AX・辞書を用途別に整理

1. OCRのrawと整形後を両方残す。まず「キャン\nディ」の単語内改行だけで成功が変わるか、同じかな入力・同じAFM方式で比較する。日本語境界の連結と英語の単語間スペースを分け、段落を無条件にすべて連結しない。
2. 取得元・アプリ・ウィンドウ・時刻と、使った短い語彙候補を区別する。画面全体の文章を無条件に一次資料とみなさず、現在の入力先に関連する語を優先する。
3. ユーザー辞書へ「表記・読み・用途」を持たせ、ASRヒントと補正で同じ候補を共有する。読み不明・多義語は無理に変換しない。未登録語や画面にない語の推測も抑える。
4. Whisperには短い日本語の語彙・局所文脈を与える。かな表記の命令を付ければ従うとは扱わない。現行ヒント、語彙のみ、ヒントなしの対照を残す。

### 画像の最終試験：ローカルVLMを独立条件として追加

現在の画像全体／34領域の分類は残し、画像を直接扱えるローカルVLMで、見えている物体・動作・読める文字を抽出する条件を追加する。その結果だけを補正モデルの候補選択へ渡す。CIで動作を測れるQwenを先に評価し、AFMは実機比較の追加条件にする。まずは常時実行でなく、OCRが空・文字文脈が不足する場合の明示オプションにする。

VLMへの質問には「雨か飴か」「正解は飴」といった試験の答えを入れず、発話も渡さず画像の観察を得る。全体画像と固定領域の処理を記録し、画像が分からない場合は未判定とする。補正モデルへの本文・読み・画像観察を分離する。これはOpenWhisprの画像対応モデルへのルーティングを参考にした独自のローカル比較案であり、特定モデルの合格を保証するものではない。モデルと実行基盤は、Mac上の対応・必要メモリ・モデル配布条件・実測を確認して選ぶ。

## 3. PR分割と合格条件

- PR A：限定置換・保護語・人名／宛先欠落の回帰試験、原文復帰。まず田中さんの削除を防ぐ。
- PR B：OCRの生／整形後、読み付き辞書、Whisperヒントの比較。モデルを同時に変えない。
- PR C：ローカルVLMの準備と画像観察のJSON、既存の全体／領域分類との比較。画像認識の改善と補正の改善を別々に採点。

各PRで計測する項目：全文一致、同音語選択、数値・名前・宛先・否定・動作・語尾の保持、文脈の混入、逆文脈での誤誘導、ガード棄却、モデル実行失敗、ロード／取得／推論時間、メモリ。期待語が出ただけ・ガードを通っただけ・原文へ戻っただけをモデルの生成成功に数えない。

雨／飴まきの同じ音声2反復に加え、別の実話者と複数の同音語、意味のある繰り返し、長文、無関係な画面を含める。測定時の出力上限やモデル再利用を製品条件と揃える。追加学習は、これらの失敗例と人手の正解が蓄積してから検討し、初手でモデルを大型化／微調整することは推奨しない。
