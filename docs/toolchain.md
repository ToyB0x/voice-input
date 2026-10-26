# miseで開発ツールを準備する

macOS 27以上の開発環境はmise 2026.10.18以上を使います。Node／pnpm／Python／CMakeは`.mise/config.toml`、Rustは`rust-toolchain.toml`で固定しています。

| ツール | 版 | 用途 |
|---|---|---|
| Node.js | 26.12.1 | フロントエンドとTauri CLI |
| pnpm | 12.15.0 | 固定ロックの依存導入・スクリプト実行 |
| Python | 3.12.14 | モデル準備・ビルド補助・依存公開日の検査 |
| CMake | 4.4.4 | Whisper／Qwenのネイティブランタイムのビルド |
| Rust | 1.99.1 | アプリとヘルパー。minimal＋rustfmt／clippy |

pnpmはmiseの`npm:pnpm`バックエンドで公式npm配布を使用します。Rustはmiseの組み込みバックエンドがrustupを用意し、既存のRust設定ファイルにあるコンポーネントも導入します。Rustの版をmise側へ重複記載しません。

## 導入

HomebrewがあるMacで、mise自体は初回のみ導入します。

```sh
brew install mise
```

リポジトリのディレクトリで設定を確認・信頼してからツールを導入します。

```sh
cd voice-input
git pull --ff-only
mise trust
mise install  # mise i でも同じ
mise exec -- python3 scripts/check-toolchain.py
mise exec -- pnpm install --frozen-lockfile
```

`mise install`はツールだけを導入します。アプリの依存導入、モデル取得、設定保存、署名キー生成は別の明示操作です。既存の鍵やGitHubの設定を上書きする処理はありません。

シェル設定を変更せず使う場合は`mise exec --`を付けます。通常の`pnpm`／`cargo`を使う場合は、zshで次の行を実行します。毎回有効にする場合はこの行を`~/.zshrc`へ一度追加してください。

```sh
eval "$(mise activate zsh)"
```

nvmなどの他のツール管理とPATHが競合する場合も、`mise exec --`でこのプロジェクトの指定を選べます。

## アプリを開発・ビルドする

Macアプリのビルドには、別途Xcode 27以上またはmacOS 27 SDKを含むCommand Line Toolsが必要です。Swift、Clang、MakeなどはそのAppleツールチェーンを使います。OSの権限承認、Xcode／SDK、Homebrew・mise自体はプロジェクトの`mise install`の管理対象外です。Ninjaはこのビルドの必須ツールではありません。

```sh
xcrun --sdk macosx --show-sdk-version  # 27以上
mise exec -- pnpm run check:dependencies
mise exec -- pnpm run tauri dev
# 個人利用の.appを作成
APPLE_SIGNING_IDENTITY="-" mise exec -- pnpm run tauri build --bundles app --ci -- --locked
```

Linuxでもツール導入とフロントエンドのビルドは検証します。macOSのアプリやAppleモデルのビルドはMacで行います。Linuxのネイティブビルド用WebKitGTKなどのシステム依存は別途必要です。

## 更新用署名キーの生成

鍵が既にあれば再生成せず、その鍵を使ってください。以下は未設定の場合の初回操作です。Tauri CLIの鍵生成にはアプリのビルドやXcodeは不要です。

```sh
SIGN_DIR="$HOME/Library/Application Support/Voice Input Signing"
umask 077
mkdir -p "$SIGN_DIR"
mise exec -- pnpm exec tauri signer generate --write-keys "$SIGN_DIR/updater.key"
```

対話でパスワードを設定し、秘密鍵・公開鍵・パスワードを手元でバックアップしてください。Tauriが作るファイルは既にTauri用のBase64形式で、追加の変換はしません。

| GitHubの登録先 | 名前 | 値 |
|---|---|---|
| Actions Secret | `TAURI_SIGNING_PRIVATE_KEY` | `updater.key`の内容 |
| Actions Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成時のパスワード |
| Actions Variable | `TAURI_UPDATER_PUBLIC_KEY` | `updater.key.pub`の内容 |

Macの`pbcopy < "$SIGN_DIR/updater.key"`で秘密鍵をコピーしてSecretへ貼り付け、次に`pbcopy < "$SIGN_DIR/updater.key.pub"`で公開鍵をVariableへ登録できます。GitHub CLI認証は不要です。秘密鍵やパスワードをチャット・コミットに含めないでください。

登録後に`v0.0.1`のGitHub Releaseを公開すると、既存workflowがビルド・署名・添付します。手順は[GitHub Releasesとアプリ内更新](../README.md#github-releases-とアプリ内更新)を参照してください。Developer ID・公証は引き続き先送りします。

## 指定版と公開後待機

Node／pnpmは既存CIとの互換性のため`.nvmrc`／`package.json`にも指定を残し、`scripts/check-toolchain.py --static`で一致を検査します。Renovateではこれらの更新を同じグループへまとめます。依存の2日間の公開後待機とセキュリティ修正の例外規則は変更しません。ツールは完全な版番号で固定し、更新は設定変更とCIを通して行います。
