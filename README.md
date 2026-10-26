# Voice Input

Typeless のように、話した内容を他のアプリへ入力できる macOS アプリを目指すプロジェクトです。
**Tauri 2 + Rust + TypeScript / Vite** で、マイクのローカル録音を実装しています。
録音からローカル日本語文字起こし、結果表示・コピーまでをアプリ内で利用できます。設定で有効にすると、入力先を再確認して他アプリへ直接入力できます。認識後の自動補正・文章整形も任意で有効にできます。

画面は「音声入力」「モデル」「設定」「比較・診断」の4タブに分かれています。初回は「モデル」で準備し、普段は「音声入力」を使います。[画面の使い分け](docs/navigation.md)を参照してください。
[直接入力の操作と権限](docs/direct-insertion.md) を参照してください。

メニューバー常駐・録音ショートカット・設定保存に対応します。
**個人利用・Developer ID／公証なしの手順は [利用ガイド](docs/user-guide.md) を参照してください。**

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

入力欄の周辺テキストを使う実験的な認識補助は実装済みです。入力欄のOCRとローカル最終補正プレビューも実装済みです。Apple Foundation Modelsによる最終補正も実験的に選択できます。対応実機での推論・品質は確認待ちです。[利用・診断手順](docs/apple-foundation-models.md)を参照してください。[自動補正](docs/automatic-correction.md)と、任意の[Qwenモデル再利用・処理時間表示](docs/correction-reuse.md)も利用できます。画面全体の文脈解析は今後の実装方針です。
詳細は [技術選定メモ](docs/architecture.md#ローカル処理と画面コンテキスト) を参照してください。

## ローカル録音

1. macOS アプリで「録音を開始」を押すと、未確認の場合に限り OS がマイクの許可を求めます。
2. 許可すると、macOS の既定の入力デバイスで録音し、入力レベルと録音時間を表示します。
3. 「停止」で音声を Rust 側のメモリに保持します。「破棄」、次の録音、アプリ終了で消去します。

録音は最大 60 秒で自動停止し、音声のファイル保存・再生・外部送信は行いません。
拒否済みの場合は録音せず、マイクのシステム設定を開くボタンを表示します。
制限された権限、入力デバイスなし、非対応形式、デバイスのエラーも画面で案内します。
音声はモノラル PCM としてメモリ内に保持し、UI へは音声を渡さずレベル・時間・状態・認識結果のみを渡します。

## 録音から文字起こし

アプリでローカルモデルの絶対パスと Metal / CPU を指定して読み込むと、録音停止後に自動で認識します。
モデルを保持して再利用し、結果をコピーできます。処理中も破棄でき、古い結果は再表示しません。
[操作方法・一時データの保持・CIの検証範囲](docs/recording-transcription.md) を参照してください。

## ローカル文字起こしエンジン

whisper.cpp / whisper-rs を使い、ローカルモデルで日本語 WAV を文字起こしできます。
モデルはアプリの明示操作からも準備できます。[モデル準備・プリセット](docs/model-setup.md)を参照してください。モデル準備と推論は別の処理です。Mac CI でネットワークを拒否した状態の推論を検証します。
導入手順・モデルの利用条件・入力形式・検証範囲は [エンジンの説明](docs/local-transcription.md) を参照してください。

## 日本語モデルの比較

base / small を固定した人間の読み上げ音声と無音で比較し、文字誤り率・時間・プロセス最大RSSを記録します。
モデルは明示的に準備し、Mac CIの推論は通信を拒否して実行します。
この小規模な読み上げ評価では、精度重視の暫定候補はsmall、速度・メモリ重視はbaseです。
[実測結果・制約・再実行手順](docs/model-evaluation.md) を参照してください。
アプリには読み込んだ重み由来のモデル名とファイル容量を表示します。

## macOS で開発する

- macOS 27 以降（アプリの対象下限。アプリCIは `xcode-27` / Apple Silicon）
- Xcode 27以降またはmacOS 27 SDKを含むCommand Line Tools（`xcrun --sdk macosx --show-sdk-version`で27以降を確認）
- [mise](https://mise.jdx.dev/) 2026.10.18以降（初回のみ `brew install mise`）

[`.mise/config.toml`](.mise/config.toml)でNode.js 26.12.1・pnpm 12.15.0・Python 3.12.14・CMake 4.4.4を固定します。Rust 1.99.1とrustfmt／clippyは既存の`rust-toolchain.toml`をmiseが読み込みます。Xcode／SDKはmiseではインストールしません。

```sh
git clone https://github.com/ToyB0x/voice-input.git
cd voice-input
mise trust
mise install  # mise i と同じ。ツールの導入だけで、依存導入や鍵生成はしません。
mise exec -- python3 scripts/check-toolchain.py
mise exec -- pnpm install --frozen-lockfile
mise exec -- pnpm run check:dependencies
mise exec -- pnpm run tauri dev
```

シェルを有効化済みなら、`mise exec --`を付けずに`pnpm`／`cargo`を使えます。macOS標準のzshでは`eval "$(mise activate zsh)"`を現在のTerminalで実行し、継続利用する場合は同じ行を`~/.zshrc`へ追加します。シェルを変更しない場合も、上記の`mise exec --`で固定版を使用できます。

初回はmiseがツールチェーンを、pnpm／Cargoが依存をダウンロードします。詳しい範囲と署名キー生成までの手順は[開発ツールのセットアップ](docs/toolchain.md)を参照してください。
Vite とネイティブウィンドウが起動し、初期画面に「Voice Input」と表示されます。
アプリを終了すると開発サーバーも終了します。

```sh
# TypeScript の型検査 + フロントエンドのビルド
pnpm run build
# Rust のフォーマット・静的解析
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
# 合成音声によるバッファ上限・変換・入力レベル・状態遷移のテスト
cargo test --manifest-path src-tauri/Cargo.toml --locked
# ローカル確認用の未署名 .app（配布用署名・公証は別途必要）
pnpm run tauri build --bundles app
```

生成先は `src-tauri/target/release/bundle/macos/Voice Input.app` です。
マイク用途の説明文と音声入力 entitlement を設定済みです。権限は録音操作時にだけ要求します。
任意の直接入力・入力欄による認識補助にはアクセシビリティ権限を使います。画面収録権限は要求しません。
現段階では API キーや外部サービスの設定は不要です。

## Linux / クラウド環境

UI のみを確認する場合は `pnpm install --frozen-lockfile` → `pnpm run dev`、ビルド確認は `pnpm run build` を実行します。
これはブラウザー用 UI で、macOS ネイティブアプリの動作確認とは別です。
Linux 上で Tauri 自体をコンパイルする場合は、Rust に加えて WebKitGTK などの
[システム依存](https://v2.tauri.app/start/prerequisites/#linux) が必要です。
macOS の `.app` のビルド・起動には Mac が必要です。

## CI

PR と `main` への push 時に macOS 上で pnpm 依存導入、Rust のフォーマット・Clippy、
音声処理の Rust テスト、評価器の Python テスト、モデル不要の固定補正ガード32件、
Tauri の `.app` ビルド、短いオフライン認識、モデル読込後を含む正常終了・資源解放の確認を実行します。
Previewの画面検証と権限・録音・OCR・直接入力の完全テストは独立した手動・週次workflowへ、全モデル比較は手動・週次の別workflowへ分けます。
[実行範囲・週次スケジュール・対象ブランチの手動検証](docs/ci.md) を参照してください。
独立した `macOS Preview UI` workflowでは、ビルドした `.app` を起動してウィンドウを撮影し、Apple Vision のローカル OCR で
「Voice Input」と本文の「Rust」「Tauri」が表示されていることを検証します。
プロセスの起動やウィンドウの存在だけでは成功にせず、本文が描画されない場合や
画面撮影に失敗した場合も、そのworkflowを失敗させます。通常CIの成功はGUI検証の成功を意味しません。
ログ保存用の `tee` が失敗を隠さないよう、CI のシェルを明示的に Bash に指定しています。
さらに、不正な Swift ソースと存在しないアプリを使った失敗ケースを実行し、
コンパイラー・検証ツールの終了コードがパイプラインから伝わることも確認します。

完全テストの録音 UI 検証では、CI のみで BlackHole 仮想マイクと合成音声を使用します。
Launch Services 経由で通常のアプリとして起動し、`tccutil` でテスト用のマイク許可をリセットして、
OS の実際の許可ダイアログを UI 操作します。許可・拒否を別々に確認し、許可後は録音・
入力レベル・停止・破棄まで検証します。製品側に CI 専用の録音・権限バイパスは設けません。
これらの仮想デバイスや権限リセットは使い捨ての GitHub ランナー内だけで行います。

通常CIの **Artifacts → `macos-build-exit-1`** には、`exit/summary.json`と終了・資源解放のログを保存します。

GUI workflowの実行結果の **Artifacts → `macos-window-smoke-1`**
（末尾は再実行回数）から次の確認資料をダウンロードできます。

- `window.png`: 実際のネイティブウィンドウのスクリーンショット
- `ocr.json` / `result.json`: 読み取ったテキストと判定結果
- `app.log` / `app.stdout.log` / `capture.log` / `smoke.log` / `swift-build.log`: 起動・撮影・検証ログ
- `windows.json`: 対象アプリの可視ウィンドウ情報
- `expected-failures/`: 終了コードの回帰チェックで意図的に起こした失敗のログ・判定結果
- `coverage.json`: ビルド・終了 / 画面スモーク / 完全結合テストの要求範囲
- `permission-denied/` / `permission-allowed/`（完全テスト時）: 実際の許可ダイアログ、録音状態の画像、UI 操作の検証結果

失敗時も、その時点までに生成できた資料を保存します。保存期間は 14 日です。
完全テストでは、録音→日本語認識→結果表示→実際のクリップボードへのコピーと、認識中の破棄も検証します。
`transcription/` に画面・認識結果・判定を保存します。
CI では仮想入力デバイスと合成音声を検証します。物理マイク固有の音質・接続切替、
他アプリへの入力、対応するすべての OS / 実機構成は別の検証対象です。
配布用署名・公証・リリースは行いません。

## 構成

```text
crates/local-transcription/  独立したローカル文字起こしライブラリ・CLI
src/                  TypeScript / CSS の初期画面
src-tauri/src/        Rust のデスクトップアプリ起動処理
src-tauri/            Tauri 設定・Cargo.lock・アイコン
docs/architecture.md  フレームワーク選定と後続機能
```

## 入力欄による認識補助（実験的）

設定で有効にすると、録音開始時の別アプリの入力欄からカーソル周辺だけを取得し、ローカルWhisperの認識ヒントに使います。初期状態は無効で、直接入力とは独立しています。取得できない場合は通常の音声認識を継続します。テキストの保存・ログ出力・クラウド送信はしません。権限、取得範囲、再試行や認識精度の制約は[認識補助ガイド](docs/recognition-context.md)を参照してください。

## ローカルOCR・最終補正

入力先の欄をApple VisionでOCRし、取得テキストのWhisper／最終補正への利用を独立して切り替えられます。軽量なローカルQwen3による補正はプレビュー専用で、元の文字起こしを保持します。初期状態では取得元は無効、補正モデルは未設定です。[準備・権限・比較方法](docs/local-correction.md)を参照してください。macOS 27以降では、[Appleのオンデバイスモデルによる補正](docs/apple-foundation-models.md)も実験的に選べます。

### 実機での一括比較

「実機で一括比較」を開き、同じ録音に対する音声モデル・CPU/Metal・Qwen/Apple Foundation Models/補正なし・AX/OCR・文脈の利用先（Whisperのみ／補正のみ／両方／なし）を選択します。「すべてのパターン」でカタログと現在設定中のモデルの全組み合わせを選べます。重複入力は除外され、未準備モデル・利用不可のAppleモデル・取得できなかった文脈はスキップして理由を表示します。モデル取得は既存の準備画面で明示的に行います。

画面文脈を比較するには、設定でAX/OCRの取得元を有効にして保存し、「次の録音から比較用文脈を保持する」を有効にした後、対象アプリでショートカット録音します。比較用の保持設定は再起動でオフになります。録音時の同じ文脈を全ケースへ使い、比較中に再取得しません。無効化・破棄・新規録音・終了で消去します。

実行は直列です。通常のWhisperモデルを解放した後、各条件で子プロセスを起動し、同じメモリ内PCMをパイプ経由で渡します。各子を終了・回収してから次へ進むので、比較モデルを同時に複数保持しません。ASRは最大600秒、補正は最大150秒で、全件選択は非常に長時間になる可能性があります。中止・破棄・終了時は実行中の子を終了・回収します。通常入力を再開するときは音声モデルを読み込み直してください。

表示時間にはモデル読み込み・推論・IPC・子の終了待ちが含まれ、カタログモデルの事前サイズ・SHA-256検証は含みません。各試行でモデルは読み直しますが、OSキャッシュ・実行順序・温度の影響は残るため、厳密な冷間ベンチマークではありません。Metalを実際に初期化できなければ利用不可として扱います（デジタル無音は推論自体を省略します）。

任意の基準文を入力すると、NFKC・小文字化・空白と句読点の除去後の文字誤り率（CER）を認識・補正候補・最終出力それぞれで比較できます。基準文はモデルへ渡しません。CERは小さいほど誤りが少なく、100%を超えることもあります。基準文なしでは精度の数値を作らず、意味と書式は本文を比較してください。原文保持チェックで棄却された補正候補も表示し、最終出力にはWhisper原文を保持します。比較結果は自動入力しません。

比較レポートはボタン操作でJSONをクリップボードへコピーできます。標準では条件・時間・CER・失敗理由のみで、本文は追加チェックを選択した場合だけ含みます。音声・画面文脈・基準文・モデルパスは含めません。本文を含むレポートを共有するときは内容を確認してください。

### GitHub Releases とアプリ内更新

Tauri公式の`tauri-plugin-updater`を利用します。署名キーを設定した公開ビルドは起動後と6時間ごとに、録音・推論・比較・モデル準備中を避けて公開Releaseの`latest.json`を確認します。確認が処理開始と重なる場合は手動で再試行できます。GitHubへ送るのは公開更新情報・アプリ本体のHTTP要求のみで、音声・本文・文脈は送りません。更新の適用は「更新をインストールして再起動」の明示操作です。署名と署名内のバージョンが一致することを公式プラグインで検証し、録音・認識結果・補正結果・比較結果が残っていれば適用を拒否します。必要な結果をコピーし、録音と比較を破棄してから実行してください。

初回のみ、リポジトリ所有者としてGitHub CLIにログインしたMacで以下を実行します。通常のリポジトリアクセスとは別に、Actions SecretsとVariablesの管理権限が必要です。

```sh
pnpm install --frozen-lockfile
python3 scripts/setup-updater-signing.py --key-file "$HOME/Library/Application Support/Voice Input/updater.key"
```

このスクリプトは公式Tauri CLIで鍵を生成し、秘密鍵を`TAURI_SIGNING_PRIVATE_KEY`（Actions Secret）、空の鍵パスワードを`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（Secret）、公開鍵を`TAURI_UPDATER_PUBLIC_KEY`（Actions Variable）へ設定します。秘密鍵は指定したリポジトリ外の場所へ0600で保持し、出力・コミットしません。既存ファイルや既存のリポジトリ設定を検出したら上書き・鍵交換せず停止します。秘密鍵のバックアップを保管してください。途中で設定に失敗した場合も鍵ファイルは残るので、新しい鍵に交換せず既存の鍵で設定を完了してください。鍵のローテーションは旧アプリからの更新を壊すため、このスクリプトでは行いません。

`v0.0.1`などの安定版SemVerタグでGitHub Releaseを公開すると、`Build and publish macOS release`がApple Silicon向けアプリをビルドします。Tauri生成の更新用`.app.tar.gz`と署名、初回インストール用`.dmg`、`latest.json`、SHA-256一覧を同じReleaseへ添付します。アップロードにはCIの`GITHUB_TOKEN`だけを使い、アプリにGitHubトークンは埋め込みません。署名設定がない公開Releaseは失敗させます。アーカイブの実データを公式プラグインで検証し、改変データ・改変バージョン・不正署名の拒否も確認してから添付します。既存の同名添付は上書きしません。

ワークフローの手動実行は**一時キーによるビルド検証のみ**です。Releaseを公開・添付せず、Actions Artifactへ成果物を保存します。一時キー版は公開版の更新経路のテストには使わないでください。通常のPR CIではDMG生成や全モデル推論を実行しません。

当面は個人利用を前提とし、Developer ID証明書・Apple公証は行いません。更新用の署名はAppleの署名・公証とは別です。初回はReleaseのDMGを開き、Voice Input.appを`/Applications`へコピーし、DMGを取り出してから起動してください。Gatekeeperで止まる場合は既存の未公証アプリ手順に従い「システム設定 → プライバシーとセキュリティ → このまま開く」を使います。OS全体のGatekeeperを無効化しないでください。更新後もOSの確認や権限の再許可が必要な場合があります。Intel Mac向け配布とAppleの署名・公証は後続対応です。

通常の依存更新は公開後48時間以上経過した版を対象にし、確認できたセキュリティ修正は版・チェックサム・理由・期限を記録した例外で優先します。pnpmの厳格なminimum release ageと、Rustの全ロックファイルの公開日時検査を適用します。詳細と手動更新手順は[依存更新ポリシー](docs/dependency-policy.md)を参照してください。

実機でのApple／Qwen比較と文脈利用先の選び方は[実機比較ガイド](docs/real-device-comparison.md)を参照してください。

録音や画面取得なしでモデルの生成を確かめる場合は、[固定サンプルの補正自己診断](docs/correction-selftest.md)を利用できます。

認識後の補正と入力を一連で行う場合は、[自動補正と最終結果の入力](docs/automatic-correction.md)を参照してください。自動補正は初期状態で無効です。

設定画面を閉じて録音・認識・補正の進み具合を確認する場合は、[録音ステータスの小型表示](docs/status-overlay.md)を有効にできます（初期状態は無効）。
