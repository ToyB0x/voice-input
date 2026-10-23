# フレームワーク選定

## 要件と比較

macOS 向けの音声入力アプリを Rust で開発する。将来的にはメニューバー常駐、
グローバルショートカット、録音、文字起こし、文章整形、他アプリへの入力を扱う。
参考: [Typeless](https://www.typeless.com/ja)。今回は初期セットアップに絞る。

| 候補 | 特徴 | 今回の判断 |
| --- | --- | --- |
| Tauri 2 | Rust バックエンドと OS の WebView。トレイやショートカットの公式 API / プラグインがある | 採用。小さな設定 UI から始め、OS 連携を Rust 側に追加できる |
| GPUI | Rust で GPU ベースの UI を記述できる | 今回の小さな UI に対して UI・OS 連携の実装判断が増える |
| Iced | Rust で宣言的な UI を記述できる | UI も Rust に統一できるが、常駐アプリの OS 連携を別途組み立てる必要がある |

**Tauri 2 + Rust + vanilla TypeScript / Vite** を採用する。
UI は HTML / CSS / TypeScript、OS 連携・音声処理は Rust が担当する。
Tauri は SwiftUI のようなネイティブウィジェット UI ではない。
現段階では React などの UI フレームワークや不要なプラグインを導入せず、
npm / Cargo のロックファイルで依存を管理する。

## 今回の範囲

- デスクトップウィンドウと日本語の初期画面
- 開発・ローカル `.app` ビルド用コマンド
- Node / Rust のバージョン指定、macOS CI、開発手順
- リモートコンテンツを読み込まない CSP と、追加 IPC 権限を持たない初期構成

## 後続の実装で決めること

1. トレイ常駐とグローバルショートカット。利用する API のみ権限を追加する。
2. Rust 側のマイク録音・デバイス選択・状態表示。
   `NSMicrophoneUsageDescription` と macOS の許可フローを整える。
3. 音声認識をローカルで行うか外部 API に送るか。
   データ保存・送信の仕様とコストを決めてからエンジンを選ぶ。
4. 文章整形と他アプリへの入力。
   macOS Accessibility API を使う場合の許可・入力先・失敗時の挙動を設計する。
5. Developer ID による署名・公証と配布。

Tauri 自体が文字起こしや他アプリへの入力を提供するわけではない。
Rust クレートや macOS API 連携で実装し、Mac 実機で検証する。

## 参考

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri architecture](https://v2.tauri.app/concept/architecture/)
- [System tray](https://v2.tauri.app/learn/system-tray/)
- [Global shortcut](https://v2.tauri.app/plugin/global-shortcut/)
- [GPUI](https://www.gpui.rs/)
- [Iced](https://iced.rs/)
