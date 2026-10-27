# 未採用の辞書補正実験を保存する

PR [#45](https://github.com/ToyB0x/voice-input/pull/45)・[#47](https://github.com/ToyB0x/voice-input/pull/47)・[#49](https://github.com/ToyB0x/voice-input/pull/49)の実験を再検討できるよう、固定評価ケースと最終v5の実測集計を保存する。既存の有効な回帰テストは削除・変更していない。このディレクトリ自体はCIの合格を意味せず、通常アプリへ試作補正器を導入しない。

- `replacement-cases.json`: 43ケース。42ケース×2回を採用ゲートとし、真の場面を文脈から識別できない1ケース×2回は別集計。
- `replacement-confirmation-cases.json`: 別の同音語等を含む14ケース×2回。固定の合成テキストであり、独立した実話者の音声ではない。
- `results-summary.json`: GHA artifactの `replacements/report.json` から抽出した集計・不一致例。原レポート、ケース、モデル、バイナリのSHA-256とソースコミット・run URLを記録する。決定的な原文保持とモデル推論を分けて集計する。

再実行する場合は、各レポートのソースコミットにある `scripts/evaluate_replacements.py` と `.github/workflows/correction-quality.yml` を参照する。現行mainに同名の評価器があるとは限らない。閉じたPRとコミットを参照元として残す。

14B確認セットのレポートは28/28・`quality_pass: true`。ただしrun 40499631479自体は最終集約ジョブの `Missing model report` で失敗した。レポート単体の結果とワークフローの成功を区別し、全体セットの80/84を覆す根拠にはしない。

画像結合の旧試験とLTの結果は[評価記録](../../../docs/accuracy-first-results.md)を参照する。
