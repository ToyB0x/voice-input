# CIの実行範囲

通常のPRと`main`へのpushでは、モデルの全件比較を起動しません。ビルド・回帰テスト・正常終了と、短いbaseモデルのオフライン認識を実行します。ウォームキャッシュでは10分未満を目標にします。初回のネイティブ依存ビルド、依存更新、GitHubランナーの待機には追加の時間がかかる場合があります。

| 実行契機 | Workflow | 検証内容 |
|---|---|---|
| PR / main push | CI | TypeScriptビルド、Rust fmt・Clippy・単体テスト、Pythonの評価器テスト、モデル不要の固定ガード32件、同梱ランタイムのモデル不要起動、モデル未読込・CPU/Metal指定でモデル読込後のネイティブ終了要求と資源解放、短いbase認識の通信禁止テスト、Metal利用可否 |
| 毎週日曜18:07 UTC / 手動 | macOS Preview UI (`full_integration=true`) | アプリのビルド・終了検証に加え、実画面撮影・表示文字のOCR、実OS権限の許可・拒否、仮想マイク、録音・認識・コピー・破棄、OCR・補正、文脈取得8ケース、直接入力11ケース、終了時の子プロセス回収 |
| 毎週火曜18:17 UTC / 手動 | Model evaluation | base/smallの比較と、1.7B/4B/8Bの文脈4条件・補助ケース・変更前後の比較 |
| 毎週木曜18:27 UTC / 手動 | Model preset evaluation | medium/turboと4Bの組み合わせ、文脈4条件、補正前後のCER |
| 手動 | Stronger local model capabilities | より大きなWhisper・8Bの診断。実験的Metalは明示指定時のみ |
| 対象Swift / workflow変更のPR、または上記手動診断 | Stronger local model capabilities | Apple Foundation Modelsの利用可否検査。PRで大モデルの推論は行わない |

週次実行は既定ブランチを対象とします。重いworkflowの曜日を分け、通常CIとモデル比較がMacランナーを同時に取り合う頻度を減らします。通常CIの古いコミットは新しい実行で中断します。手動・週次の完全テストは別のconcurrencyグループで実行し、通常のpushによって中断されません。

## 対象ブランチの完全テスト

権限・録音・文脈取得・直接入力・補正プロセスやCIの完全テストを変更した場合は、対象ブランチで完全テストを実行して結果を確認します。27 Previewの表示検証が阻まれる場合は未検証範囲を記録し、実機確認で補います。既存の`ci.yml`も手動の`full_integration=true`を受け付けますが、通常の手動実行はfalseが既定です。

```sh
gh workflow run macos-ui-preview.yml --ref YOUR_BRANCH -f full_integration=true
# 画面・エンジン選択・比較UIだけを検証する場合
gh workflow run macos-ui-preview.yml --ref YOUR_BRANCH -f full_integration=false
```

モデル・プロンプト・文脈の評価を必要とする変更では、対象ブランチの比較も手動実行できます。

```sh
gh workflow run model-evaluation.yml --ref YOUR_BRANCH
gh workflow run model-presets.yml --ref YOUR_BRANCH
# mediumも含む音声モデル比較
gh workflow run model-evaluation.yml --ref YOUR_BRANCH -f include_medium=true
# 大モデル単体・AFM・必要に応じたMetal診断
gh workflow run model-capabilities.yml --ref YOUR_BRANCH
```

手動の全件評価は10分を超えることがあります。各LLMで「8入力×文脈4条件＋補助28ケース」を、モデル読み込みを含むCPU処理で測定します。8Bの補助28ケースだけでも約23分かかった実行があります。単体診断は同じ補助ケースを再実行するため、必要時の手動実行に限定します。データ・モデル・推論条件・集計方法は従来どおりです。

## 画像と検証範囲

通常CIの`macos-build-exit-<再実行回数>` Artifactには`exit/summary.json`と各終了ケースのログ、OS検証結果を保存します。録音・推論中の終了や実際のGPUオフロードを証明するものではありません。

独立した実験的GUI workflowの`macos-window-smoke-<再実行回数>` Artifactには、実際の`window.png`、`ocr.json`、`result.json`と起動ログを保存します。表示文字が描画されない場合、撮影失敗、検証ツールの異常終了は失敗として扱います。

`coverage.json`の`requested_scope`は、通常CIが`build-exit-checks`、画面検証が`build-window-smoke`、完全テストが`full-integration`です。`ui_requested`も記録します。要求した範囲を表す値であり、成否は各`result.json`・`interaction.json`とジョブの結果で確認します。権限・録音・OCR・入力の操作画像や詳細結果は完全テスト時に追加されます。

権限の実操作は使い捨てランナー内で行います。製品の権限バイパスは追加せず、音声・取得テキストは公開・合成データだけを使います。推論時の通信禁止、原文保持、失敗時のArtifact保存、署名・公証を行わない方針を維持します。

完全CIでは、アプリ同梱の補正ランタイムを固定の合成文で3回起動し、処理段階ごとの時間とプロセスのピークRSSも記録します。`correction-performance/performance.json`と`summary.md`を通常のMac Artifactに含めます。通常のPR／pushでは実行しません。各実行は新しいプロセスであり、初回／後続の差はモデル常駐化の効果を示しません。計測範囲と手元での実行方法は[最終補正の計測](local-correction.md#処理時間メモリの計測)を参照してください。

通常CIにはApple Foundation Models用ヘルパーのビルドとモデル不要の検証も含めます。macOS 27 SDKでコンパイルしたヘルパーを同じrunのArtifact経由で27のアプリ検証へ渡し、ハッシュ・リビジョン・プロトコルを確認します。実画面での選択と診断コピーは独立したGUI workflowで確認します。実際のAppleモデルが使えない場合は`inferenceTested: false`を記録します。[AppleモデルのCI範囲](apple-foundation-models.md#実装とciの範囲)も参照してください。

Macジョブは、ビルドが完了した時点でアプリを `voice-input-macos-ARM64-<試行番号>` ArtifactへZIPとSHA-256・ソースリビジョン付きで保存します。モデルと個人設定は含めず、Developer ID署名・公証は行いません。実機での利用は[個人利用ガイド](user-guide.md#ciで検証したアプリを使う)を参照してください。

macOS 27のRust/CMakeビルドキャッシュは26以下のものと分けます。OpenSSLやSDKの絶対パスを含む旧OSの生成物を新OSへ復元しません。OS更新直後の初回はキャッシュがないため、通常より長くかかることがあります。
評価workflowのUbuntuランナー／ネイティブキャッシュを変更するPRでは、`Ubuntu runner compatibility`も実行します。Ubuntu 26.04で実際に補正ランタイムをビルド・テストし、通信を禁止して製品の補正ガードを検証します。モデルを取得する全件評価は起動しません。ネイティブ生成物のキャッシュはUbuntu 24.04と分離します。

通常Macジョブのタイムアウトは30分です。macOSメジャー更新や空キャッシュ時のC++・Rustの初回コンパイルを完了させるための上限であり、通常実行の10分未満という目標は維持します。完全テストの60分上限とは分けています。

## 対応OSとPreviewランナー

アプリの最低対応OSはmacOS 27です。ビルド・画面・権限検証・Release生成には、[公式の27イメージ](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md)のラベル`xcode-27`を明示します。これはPublic Previewであり、`macos-latest`が27を指すとは限りません。ホストOSとSDKを実測し、27未満なら失敗させます。配布アプリのInfo.plistと主実行ファイル・補正・Appleモデル・比較の各実行ファイルの最低OSも検証します。14専用ジョブと手動入力は削除しました。

音声エンジン単体・モデル比較のmacOS 26ジョブは補助検証として残します。26での成功は27のアプリ動作確認を代替しません。

Previewでの画面撮影・描画・権限操作は通常PR CIから独立した手動・週次workflowで検証します。失敗はそのworkflow上も失敗として残し、Artifactとログで調査します。通常CIの成功はGUI検証の成功を意味しません。実機テスト用のPRは、静的解析・ビルド結果と失敗の範囲を明記して判断できます。27での起動・画面表示が未確認なら、配布アプリの動作確認済みとは扱いません。ビルド済みアプリは後続のUIテストが失敗してもArtifactに残し、利用者が実機で検証できるようにします。

2026-10-25の27 Preview調査では、26でビルドしたアプリのウィンドウ内容が空で、表示検証は未通過でした。2026-10-26（日本時間）の[27向けビルド検証](https://github.com/ToyB0x/voice-input/actions/runs/40228715894)でも、ビルド・各単体テスト・4実行ファイルの最低OS検査は成功しましたが、画面は空で、60秒の表示検証は`Vision OCR failed: nilError`により失敗しました。原因は未特定で、Preview固有と断定しません。後続のエンジン選択・比較UIと完全な権限・録音・OCR・直接入力はこの27環境で未検証です。調査を広げず、同runのアプリArtifactと画面・ログを使って実機で確認します。Appleモデルの実APIは26・27の標準GHA環境で`deviceNotEligible`を返し、推論は未検証です。対応する実機での[確認手順](user-guide.md#対応osとciの結果)を残します。

2026-10-26（日本時間）の実機確認では、27.0.1で起動・モデル取得・外付けマイク録音・認識・コピー・入力レベル表示と、修正版の録音・認識後のCmd+Q正常終了を確認しました。[終了検証run](https://github.com/ToyB0x/voice-input/actions/runs/40256221508)では、GHAのモデル未読込・CPU/Metal指定の3ケースとも終了コード0と音声worker停止・資源解放を確認しています。同runのGUI表示検証は空画面とVisionエラーで失敗しており、原因は未特定のままです。
