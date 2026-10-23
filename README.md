# Voice Input

Typeless のように、話した内容を他のアプリへ入力できる macOS アプリを目指すプロジェクトです。
現在は **Tauri 2 + Rust + TypeScript / Vite** の開発用の土台のみです。
録音・文字起こし・文章整形・他アプリへの入力はまだ実装していません。

選定理由と今後の範囲は [技術選定メモ](docs/architecture.md) を参照してください。

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
