# Voice Input

Typeless のように、話した内容を他のアプリへ入力できる macOS アプリを目指すプロジェクトです。
現在は **Tauri 2 + Rust + TypeScript / Vite** の開発用の土台のみです。
録音・文字起こし・文章整形・他アプリへの入力はまだ実装していません。

選定理由と今後の範囲は [技術選定メモ](docs/architecture.md) を参照してください。

## コンセプト

**音声の認識・解析は Mac 上のローカル処理のみで行い、クラウドへ送信しません。**
音声認識、文脈に応じた補正、文章整形まで端末内で完結する設計とします。

入力精度を高めるため、設定フラグで次の画面コンテキスト取得を個別に有効化できる構想です。
どちらも初期状態では無効とし、有効化と macOS の権限許可を得た場合に利用します。

- **アクセシビリティ経由のテキスト取得**: Mac の Accessibility API から取得できる
  画面上のテキストを、入力先の用語や文脈の補助情報として使います。
- **画面撮影と OCR**: 画面を撮影して端末内で OCR を行い、画面上の文字を取得します。
  静的な画像・文字列処理や OCR 用のローカル AI モデルを検討し、取得したテキストは
  必要に応じて Apple の Foundation Models framework などでローカル解析します。

取得したテキストと音声情報を端末内で組み合わせ、固有名詞・専門用語の認識や文章の補正に活用します。
音声、文字起こし、スクリーンショット、画面から取得したテキスト、統合後の解析結果は
クラウドや外部 API に送信しない方針です。

これらは今後の実装方針であり、現在の初期セットアップには含まれていません。
詳細は [技術選定メモ](docs/architecture.md#ローカル処理と画面コンテキスト) を参照してください。

## macOS で開発する

- macOS 10.15 以降（Tauri 2 のサポート下限。CI は macOS 14）
- Xcode Command Line Tools: `xcode-select --install`
- Node.js 24（`.nvmrc`。npm を使用）
- [rustup](https://rustup.rs/)（Rust は `rust-toolchain.toml` で指定）

```sh
git clone https://github.com/ToyB0x/voice-input.git
cd voice-input
# nvm を使っている場合: nvm install && nvm use
npm ci
npm run tauri dev
```

初回は Rust ツールチェーンと依存クレートをダウンロードします。
Vite とネイティブウィンドウが起動し、初期画面に「Voice Input」と表示されます。
アプリを終了すると開発サーバーも終了します。

```sh
# TypeScript の型検査 + フロントエンドのビルド
npm run build
# Rust のフォーマット・静的解析
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
# ローカル確認用の未署名 .app（配布用署名・公証は別途必要）
npm run tauri build -- --bundles app
```

生成先は `src-tauri/target/release/bundle/macos/Voice Input.app` です。
マイクやアクセシビリティの権限は、対応機能を実装する段階で追加します。
現段階では API キーや外部サービスの設定は不要です。

## Linux / クラウド環境

UI のみを確認する場合は `npm ci` → `npm run dev`、ビルド確認は `npm run build` を実行します。
これはブラウザー用 UI で、macOS ネイティブアプリの動作確認とは別です。
Linux 上で Tauri 自体をコンパイルする場合は、Rust に加えて WebKitGTK などの
[システム依存](https://v2.tauri.app/start/prerequisites/#linux) が必要です。
macOS の `.app` のビルド・起動には Mac が必要です。

## CI

PR と `main` への push 時に macOS 上で npm 依存導入、Rust のフォーマット・Clippy、
Tauri の `.app` ビルドを実行します。配布用署名・公証・リリースは行いません。

## 構成

```text
src/                  TypeScript / CSS の初期画面
src-tauri/src/        Rust のデスクトップアプリ起動処理
src-tauri/            Tauri 設定・Cargo.lock・アイコン
docs/architecture.md  フレームワーク選定と後続機能
```
