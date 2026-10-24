# 大きいローカルモデルとApple Foundation ModelsのCI確認

`Stronger local model capabilities` ワークフローを手動実行できます。対応するPRでも検証します。既存のアプリ設定や標準モデルは変更しません。モデル取得だけを明示セットアップとしてネットワーク許可下で行い、推論時はMacの`sandbox-exec`またはLinuxのseccompでネットワークを禁止します。公開音声・合成テキストだけを使います。

| 候補 | 検証 | 重みのおおよその容量 |
|---|---|---:|
| Whisper medium | Mac CPUで公開日本語音声7件＋無音、CER・速度・RSS。Metalは手動オプション | 1.53 GB |
| Whisper large-v3-turbo | 同上。large-v3のデコーダーを削減した高速版 | 1.62 GB |
| Whisper large-v3 | 同上。容量と計算量が大きい候補 | 3.10 GB |
| Qwen3 8B Q4_K_M | Linux CPUで補正用の固定20ケース、実行・棄却・数値／否定／書式・時間・RSS | 5.03 GB |
| Apple Foundation Models | 最新MacランナーでSDK・OS・モデル利用可否。利用可能な場合だけ日本語の句読点補正を実行 | OS管理 |

重みの容量と実行時メモリは異なります。標準Macランナーの7 GiB環境では8Bが約84〜113秒かかり、言い直しの1ケースが製品の時間上限に達しました。標準の`ubuntu-24.04`（公開リポジトリでは16 GBメモリ）へモデルのCPU検証を移し、製品と同じ時間上限で評価します。Mac上のアプリ結合テストは従来のMac CIが担当します。異なるランナーの時間・メモリを直接の性能比較には使いません。

Macの仮想GPUではmedium / large-v3-turbo / large-v3がMetal初期化後の最初の音声で不正UTF-8を返しました。[初回検証のArtifact](https://github.com/ToyB0x/voice-input/actions/runs/40084677593)に失敗ログとCPU結果を保存しています。標準PRではCPUを必須評価にし、Metalは`NOT TESTED`と記録します。再検証する場合は手動実行の`include_experimental_metal`を有効にします。その場合もMetalが失敗すればジョブを失敗にし、エラーの置換や成功扱いは行いません。CPU推論成功とGPU推論成功は区別します。

Linuxでは`run-offline-linux.py`のseccompルールがexec・子プロセスにも継承され、socket・接続・送受信・io_uringのシステムコールを`EPERM`で拒否します。ルールを読み込めない場合は起動を中止します。ネットワーク拒否と実行時間切れ時の子プロセス停止をテストしています。8Bは`--cpu-only`を明示し、Metalを未検証と記録します。

このワークフローは実行可能性と固定ケースの制約を確認します。別の`Model evaluation`では、8Bを含む3モデルを同じLinux CPUで4条件比較し、事実・書式・旧9ケースからの変化を記録します。詳細は[補正品質の比較](correction-quality.md)を参照してください。モデルが大きいだけで認識・意味保持が改善するとは限らず、既存の原文保持・手動採用を維持します。

## Apple Foundation Models

[Apple公式の利用可否確認](https://developer.apple.com/documentation/foundationmodels/generating-content-and-performing-tasks-with-foundation-models)に従い、`SystemLanguageModel.default.availability` を調べます。macOS 26以降と対応SDKだけでは十分ではなく、Apple Intelligence対応端末・地域、有効化、モデル準備が必要です。

プローブはApple Intelligenceを自動で有効化したり、Apple Accountへサインインしたりしません。利用不可は `deviceNotEligible` / `appleIntelligenceNotEnabled` / `modelNotReady` として保存し、推論成功には数えません。SDKにFrameworkがない場合も区別します。利用可能ならオンデバイスの `LanguageModelSession` で合成テキストだけを処理します。クラウドモデルやPrivate Cloud Computeは呼びません。

標準ホステッドランナーで利用不可なら、対応する個人MacでApple Intelligenceを準備した上での実行、またはそのMacをself-hosted runnerにする方法が候補です。self-hosted runnerの導入はこの検証では行いません。Appleモデルはテキスト補正の別候補であり、Whisperに置き換わる音声認識モデルでも、必ずQwen3より高精度なモデルでもありません。

## 結果の読み方

ActionsのArtifact `stronger-whisper-*`、`stronger-correction-qwen3-8b-linux-cpu`、`apple-foundation-models-availability` を確認します。Whisperの `report.json` にCER・実行モデル・重みSHA-256、`metal-coverage.json`に未実行理由、手動のMetal評価時は別のレポートを保存します。Qwenの `report.json` に各ケースの結果とMetal未実行の状態、Appleの `report.json` に利用不可理由または実際の生成結果を保存します。AppleのSDK/Xcode/OS情報とランナーのメモリ情報も保存します。

モデルは公式配布の固定リビジョン・容量・SHA-256で照合します。Whisperのエンジンはlarge-v3とturboの両方を`large`と報告するため、正確なバリアントは重みのハッシュで検証します。公証・証明書の対応は引き続き先送りです。
