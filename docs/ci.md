# CIの実行範囲

通常のPRと`main`へのpushでは、モデルの全件比較を起動しません。ビルド・回帰テスト・実画面表示と、短いbaseモデルのオフライン認識を実行します。ウォームキャッシュでは10分未満を目標にします。初回のネイティブ依存ビルド、依存更新、GitHubランナーの待機には追加の時間がかかる場合があります。

| 実行契機 | Workflow | 検証内容 |
|---|---|---|
| PR / main push | CI | TypeScriptビルド、Rust fmt・Clippy・単体テスト、Pythonの評価器テスト、モデル不要の固定ガード32件、同梱ランタイムのモデル不要起動、実ウィンドウ撮影・表示文字のOCR、短いbase認識の通信禁止テスト、Metal利用可否 |
| 毎週日曜18:07 UTC / 手動 | CI (`full_integration=true`) | 通常CIに加え、実OS権限の許可・拒否、仮想マイク、録音・認識・コピー・破棄、OCR・補正、文脈取得8ケース、直接入力11ケース、終了時の子プロセス回収 |
| 毎週火曜18:17 UTC / 手動 | Model evaluation | base/smallの比較と、1.7B/4B/8Bの文脈4条件・補助ケース・変更前後の比較 |
| 毎週木曜18:27 UTC / 手動 | Model preset evaluation | medium/turboと4Bの組み合わせ、文脈4条件、補正前後のCER |
| 手動 | Stronger local model capabilities | より大きなWhisper・8Bの診断。実験的Metalは明示指定時のみ |
| 対象Swift / workflow変更のPR、または上記手動診断 | Stronger local model capabilities | Apple Foundation Modelsの利用可否検査。PRで大モデルの推論は行わない |

週次実行は既定ブランチを対象とします。重いworkflowの曜日を分け、通常CIとモデル比較がMacランナーを同時に取り合う頻度を減らします。通常CIの古いコミットは新しい実行で中断します。手動・週次の完全テストは別のconcurrencyグループで実行し、通常のpushによって中断されません。

## 対象ブランチの完全テスト

権限・録音・文脈取得・直接入力・補正プロセスやCIの完全テストを変更した場合は、対象ブランチで完全テストを実行して結果を確認します。

```sh
gh workflow run ci.yml --ref YOUR_BRANCH -f full_integration=true
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

`macos-window-smoke-<再実行回数>` Artifactには、通常CIでも実際の`window.png`、`ocr.json`、`result.json`と起動ログを保存します。表示文字が描画されない場合、撮影失敗、検証ツールの異常終了は失敗として扱います。

`coverage.json`の`requested_scope`が`build-window-smoke`なら通常CI、`full-integration`なら完全テストです。要求した範囲を表す値であり、成否は各`result.json`・`interaction.json`とジョブの結果で確認します。権限・録音・OCR・入力の操作画像や詳細結果は完全テスト時に追加されます。

権限の実操作は使い捨てランナー内で行います。製品の権限バイパスは追加せず、音声・取得テキストは公開・合成データだけを使います。推論時の通信禁止、原文保持、失敗時のArtifact保存、署名・公証を行わない方針を維持します。

完全CIでは、アプリ同梱の補正ランタイムを固定の合成文で3回起動し、処理段階ごとの時間とプロセスのピークRSSも記録します。`correction-performance/performance.json`と`summary.md`を通常のMac Artifactに含めます。通常のPR／pushでは実行しません。各実行は新しいプロセスであり、初回／後続の差はモデル常駐化の効果を示しません。計測範囲と手元での実行方法は[最終補正の計測](local-correction.md#処理時間メモリの計測)を参照してください。

通常CIにはApple Foundation Models用ヘルパーのビルドと軽いモデル不要の検証も含めます。macOS最新SDKでコンパイルしたヘルパーを、同じworkflow runのArtifact経由でmacOS 14アプリへ同梱します。ハッシュ・リビジョンを照合し、weak linkによる旧OS起動と利用不可の応答、実画面の選択・診断コピー、候補の共通ガードを確認します。CIで実際のAppleモデルが使えない場合は`inferenceTested: false`を記録します。詳細は[AppleモデルのCI範囲](apple-foundation-models.md#実装とciの範囲)を参照してください。

成功したMacジョブは、検証したアプリも `voice-input-macos-ARM64-<試行番号>` ArtifactへZIPとSHA-256・ソースリビジョン付きで保存します。モデルと個人設定は含めず、Developer ID署名・公証は行いません。実機での利用は[個人利用ガイド](user-guide.md#ciで検証したアプリを使う)を参照してください。

評価workflowのUbuntuランナーを変更するPRでは、`Ubuntu runner compatibility`も実行します。Ubuntu 26.04で実際に補正ランタイムをビルド・テストし、通信を禁止して製品の補正ガードを検証します。モデルを取得する全件評価は起動しません。ネイティブ生成物のキャッシュはUbuntu 24.04と分離します。
