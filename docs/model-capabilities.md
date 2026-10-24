# 大きいローカルモデルとApple Foundation ModelsのCI確認

`Stronger local model capabilities` ワークフローを手動実行できます。対応するPRでも検証します。既存のアプリ設定や標準モデルは変更しません。モデル取得だけを明示セットアップとしてネットワーク許可下で行い、推論は `sandbox-exec` のネットワーク禁止下で実行します。公開音声・合成テキストだけを使います。

| 候補 | 検証 | 重みのおおよその容量 |
|---|---|---:|
| Whisper medium | 公開日本語音声7件＋無音、CPU/Metal、CER・速度・RSS | 1.53 GB |
| Whisper large-v3-turbo | 同上。large-v3のデコーダーを削減した高速版 | 1.62 GB |
| Whisper large-v3 | 同上。容量と計算量が大きい候補 | 3.10 GB |
| Qwen3 8B Q4_K_M | 補正用の固定9ケース、実行・棄却・数値／否定／書式・時間・RSS | 5.03 GB |
| Apple Foundation Models | 最新MacランナーでSDK・OS・モデル利用可否。利用可能な場合だけ日本語の句読点補正を実行 | OS管理 |

重みの容量と実行時メモリは異なります。標準ランナーのメモリ・仮想GPU機能に制約があるため、大きいモデルでは時間切れやメモリ不足になる可能性があります。Qwen3は製品と同じ時間上限を使い、遅すぎるモデルを「利用可能」と見せるために上限を緩めません。CPU推論成功とGPU推論成功は区別します。WhisperのMetal成功から、異なる演算を使うQwenのMetal成功を推定しません。

Whisperは固定コーパスで精度を比較できますが、8Bの9ケースは実行可能性と指定制約の確認です。前PRの4条件・公開発話による補正品質評価の代わりにはなりません。モデルが大きいだけで認識・意味保持が改善するとは限らず、既存の原文保持・手動採用の方針は維持します。

## Apple Foundation Models

[Apple公式の利用可否確認](https://developer.apple.com/documentation/foundationmodels/generating-content-and-performing-tasks-with-foundation-models)に従い、`SystemLanguageModel.default.availability` を調べます。macOS 26以降と対応SDKだけでは十分ではなく、Apple Intelligence対応端末・地域、有効化、モデル準備が必要です。

プローブはApple Intelligenceを自動で有効化したり、Apple Accountへサインインしたりしません。利用不可は `deviceNotEligible` / `appleIntelligenceNotEnabled` / `modelNotReady` として保存し、推論成功には数えません。SDKにFrameworkがない場合も区別します。利用可能ならオンデバイスの `LanguageModelSession` で合成テキストだけを処理します。クラウドモデルやPrivate Cloud Computeは呼びません。

標準ホステッドランナーで利用不可なら、対応する個人MacでApple Intelligenceを準備した上での実行、またはそのMacをself-hosted runnerにする方法が候補です。self-hosted runnerの導入はこの検証では行いません。Appleモデルはテキスト補正の別候補であり、Whisperに置き換わる音声認識モデルでも、必ずQwen3より高精度なモデルでもありません。

## 結果の読み方

ActionsのArtifact `stronger-whisper-*`、`stronger-correction-qwen3-8b`、`apple-foundation-models-availability` を確認します。Whisperの `report.json` にCER・実行モデル・重みSHA-256・GPU利用検査、Qwenの `report.json` に各ケースの結果とMetalの利用可否、Appleの `report.json` に利用不可理由または実際の生成結果を保存します。AppleのSDK/Xcode/OS情報とランナーのメモリ情報も保存します。

モデルは公式配布の固定リビジョン・容量・SHA-256で照合します。Whisperのエンジンはlarge-v3とturboの両方を`large`と報告するため、正確なバリアントは重みのハッシュで検証します。公証・証明書の対応は引き続き先送りです。
