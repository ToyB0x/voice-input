# 今回の依存更新でのセキュリティ確認

2026-10-25時点の上流情報を確認しました。通常の更新は48時間待機しますが、確認できたセキュリティ修正を待機のために遅らせません。例外を適用しても、公開日時とチェックサムの検査は省略しません。

| 対象 | 確認した内容 | 今回の扱い |
|---|---|---|
| Node.js 26.12.1 | [公式の2026年10月セキュリティリリース](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases)に、BrotliのCVE-2026-111497への対応が記載されています。 | 修正版へ更新済み。既に48時間を経過しています。 |
| brotli-decompressor 6.2.0 | [上流の変更記録](https://github.com/dropbox/rust-brotli-decompressor/blob/072acf0f7b7514144e90eaabf2d46d4d0617d94e/README.md#whats-new-in-version-620)に、C版でCVE-2026-111497を起こす入力に対して、正しいエラーコードを返す修正が明記されています。 | セキュリティに関係する入力処理の改善として即時更新。正確な版・SHA-256・根拠・期限を記録した例外を適用します。 |
| brotli 9.1.0 | [公開版の依存指定](https://github.com/dropbox/rust-brotli/blob/73e6947c2937be7b694ce284d4384388fbe763c2/Cargo.toml)がbrotli-decompressor >=6.2を要求します。 | 上記の修正をTauriの依存へ取り込むため、同じく版限定の例外で即時更新します。 |
| erased-serde 0.4.11 | [版間の変更](https://github.com/dtolnay/erased-serde/compare/0.4.10...0.4.11)は、collect_strのストリーミング性能改善やテスト・CI更新でした。該当版への脆弱性修正の告知は確認できませんでした。 | 通常の48時間待機を維持します。 |
| smallvec 1.16.4 | [版間の変更](https://github.com/servo/rust-smallvec/compare/v1.16.3...v1.16.4)はfuzzerのテスト変更でした。 | 通常の48時間待機を維持します。 |

CVE-2026-111497の[一次情報](https://github.com/CVEProject/cvelistV5/blob/main/cves/2026/111xxx/CVE-2026-111497.json)はGoogleのC版デコーダーに関するものです。Rust版に同じリモートコード実行の脆弱性が確認された、という説明にはしません。Node.jsの修正はバックポートされているため、同梱ライブラリのバージョン文字列だけで修正の有無を判断せず、Node.js自身のセキュリティリリース情報を確認しています。

## その他のロックファイル内の既知項目

OSVで全531版を照会した結果、glib 0.18.5（[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)）、proc-macro-error 1.0.4のメンテナンス終了、zip 4.6.1（[RUSTSEC-2026-0367](https://rustsec.org/advisories/RUSTSEC-2026-0367.html)ほか）の通知も確認しました。

これらは`cargo tree --manifest-path src-tauri/Cargo.toml --locked --target aarch64-apple-darwin -i <package>`で、現在配布するmacOSの依存グラフには含まれないことを確認しています。zipはupdaterのWindows用依存、glib系はGTK用の依存としてロックファイルに残ります。上流の要求する互換範囲を超えた版を強制してビルドを壊すことはせず、macOS以外をサポートする場合に改めて対応します。ロックファイル全体の「脆弱性がゼロ」という検証結果ではありません。
