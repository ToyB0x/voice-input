# Apple Foundation Modelsによる最終補正（実験的）

Whisperによるローカル文字起こしの後に、Appleのオンデバイス`SystemLanguageModel.default`で文章を補正する仮実装です。Qwen3との選択式で、初期値は従来のQwen3を維持します。音声認識モデルをAppleモデルへ置き換える変更ではありません。

音声はWhisperでローカル処理し、Appleモデルには文字起こしと、設定で許可した同じ録音の文脈だけを渡します。クラウドの推論API、Private Cloud Compute、外部ツール呼び出しは利用しません。Apple Intelligenceの有効化やOSモデルの準備・ダウンロードは利用者とmacOSが行う操作です。アプリはそれらを自動で有効化しません。

## 実機で試す手順

1. Apple Intelligenceに対応したMacで、macOS 26以降を利用します。システム設定でApple Intelligenceを有効にし、モデルの準備完了を待ちます。日本語への対応はOS・モデルの版にも依存し、ヘルパーで`supportsLocale(ja)`を確認します。
2. 成功したCIのArtifact `voice-input-macos-ARM64-<試行番号>` をダウンロードし、`Voice-Input-macos.zip`を展開してアプリを使えます（Apple Silicon向け）。`provenance.json`にソースリビジョンとZIPのSHA-256を記録します。手元で作る場合はXcode 26以降でビルドします。署名・公証を行わない個人利用の起動手順は[利用ガイド](user-guide.md)と同じです。
3. 最終補正欄で「Apple Foundation Models（実験的）」を選び、「設定を保存」を押します。GGUFパスとCPU / Metalの指定はAppleモデルでは利用しません。実行デバイスはOSが管理します。以前のQwen設定は保存したままです。
4. 「Appleモデルの診断」を開き、「Appleモデルの利用可否を確認」を押します。この操作では推論しません。
5. いつものWhisperで録音・文字起こしを行い、「最終補正をプレビュー」を押します。初めは短い日本語で試してください。
6. 原文と比較してから「補正結果をコピー」を押します。補正結果は原文を上書きせず、自動入力もしません。既存の直接入力を有効にしている場合、それは引き続きWhisperの原文を入力する機能です。

文脈はWhisperと共通の取得・利用フラグに従います。Appleモデルの初回補正にだけ渡し、補正の再実行では文脈を渡しません。利用不可でもQwenやクラウドへ自動で切り替えず、原文を保持します。

## 診断と切り分け

利用可否は最後に明示確認した時点の値です。補正実行時にはOSの状態を改めて確認します。「利用できます」は生成成功の保証ではありません。

| 状態・固定コード | 確認すること |
|---|---|
| `available` | 次に実際のプレビューで生成・原文保持チェックの成否を確認 |
| `osTooOld` | macOS 26以降を使用 |
| `frameworkMissingFromSDK` | Xcode 26以降を選択してヘルパーを再ビルド。古いSDKで作ったヘルパーはOS更新だけでは有効にならない |
| `deviceNotEligible` | Apple Intelligence対応のハードウェアか確認 |
| `appleIntelligenceNotEnabled` | システム設定でApple Intelligenceを有効化 |
| `modelNotReady` / `assets-unavailable` | OS側のモデル準備を待ち、再確認 |
| `unsupportedLanguageOrLocale` | OS・モデル・言語設定を確認 |
| `context-window-exceeded` | 発話を短くする、または最終補正への文脈利用を無効化して再試行。入力を黙って切り詰めない |
| `guardrail-violation` | Apple側の安全制約による拒否。原文を利用 |
| `decoding-failure` / `unsupported-guide` | 構造化出力の生成失敗。OS版と診断情報を記録 |
| `rate-limited` / `concurrent-requests` | 他の処理が終わるのを待って再試行 |
| `guard-numbers` / `guard-negation` / `guard-contrast` など | Appleの生成後、共通の原文保持チェックが棄却。原文を利用 |
| `helper-missing` / `invalid-status` / `guard-runtime-failed` | アプリ内の2つの補正ヘルパーの配置とビルド手順を確認 |
| `generation-failed` | 未分類の生成失敗。利用可否を再確認し、OS版と診断情報を記録 |

「診断情報をコピー（本文なし）」で、アプリ版、選択エンジン、OS版、SDK対応の有無、利用可否、最後の処理時間、固定の失敗理由をコピーできます。音声・文字起こし・取得文脈・補正本文・モデル名のメタデータ・モデルパスは含めません。通常は診断をファイルへ保存せず、例外の`localizedDescription`など本文を含み得る情報も表示・コピーしません。診断でエラーが残る場合、設定を保存した上で、利用可否確認と実際のプレビューを分けて試すと失敗箇所を確認できます。

## ビルドと固定サンプルでの確認

```sh
# Xcode 26以降を選択した状態で実行
pnpm install --frozen-lockfile
pnpm run tauri build --bundles app
```

`prepare-native-runtime.py`は、従来のRust補正ランタイムとSwiftヘルパーをアプリへ同梱します。Swiftヘルパーの最低実行OSはmacOS 14で、FoundationModelsをweak linkします。古いOSでもアプリが起動でき、Apple補正を選ぶと`osTooOld`を返します。古いSDKでも非対応を返すヘルパーはビルドできますが、Appleモデルの生成コードを含みません。

録音と切り離してヘルパーを確認する場合（固定の合成文のみ）:

```sh
helper='src-tauri/resources/apple-foundation-correction'
"$helper" --status
printf '%s' '{"transcript":"会議は金曜日です","context":"会議、金曜日。"}' | "$helper" --correct
# モデル不要で共通の原文保持チェックを確認
printf '%s' '{"transcript":"会議は10時です","candidate":"会議は20時です。"}' | \
  src-tauri/resources/local-correction --validate
```

最後の例は`rejected`になるのが正常です。`--correct`単体は構造化した候補を返すまでのAPIであり、アプリはその後にRustの`--validate`を必ず実行します。診断コピーと異なり、`--correct`の標準出力には生成本文を含みます。

## 実装とCIの範囲

Swiftヘルパーは毎回新しい`LanguageModelSession`を作り、ツールや以前の録音の履歴を渡しません。構造化出力で本文だけを生成し、その候補を既存のRustガードで検証してからプレビューへ渡します。これらのガードは意味の同一性を保証するものではなく、Appleモデルの品質がQwenより高いことも未確認です。アプリ側の約150秒の制限と、取り消し・新しい録音・エンジン変更・終了時の子プロセス停止／回収は両エンジンに適用します。

通常CIでは、macOS最新SDKで生成コードを含むヘルパーをコンパイルし、macOS 14のアプリへ同梱します。来歴とハッシュを照合した上で、旧OSでの起動・非対応応答、サイズ制限・不正入力、共通ガード32ケース、子プロセス回収、実アプリのエンジン選択・利用可否確認・診断コピーを検証します。公開・合成データ以外をCIへ送りません。

Artifact `apple-foundation-runtime`にSDK版・来歴・利用可否の結果、通常のMac Artifactに`foundation-runtime.json`と`foundation/diagnostics.png`・`diagnostics.json`・`interaction.json`を保存します。CIのVMが`deviceNotEligible`なら`inferenceTested: false`と記録します。SDKでコンパイルできたことや模擬応答のガード試験を、実際のAppleモデルの推論成功として扱いません。対応実機での生成・日本語品質・速度はPRマージ後の確認事項です。
