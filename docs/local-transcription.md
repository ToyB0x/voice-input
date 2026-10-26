# ローカル日本語文字起こしエンジン

`crates/local-transcription` は whisper.cpp を `whisper-rs 0.16.0` 経由で使う
同期 Rust ライブラリと検証用 CLI。Tauri やマイクとは独立して動作する。
アプリの録音 UI に接続済み。操作・ワーカー・保持期間は [録音から文字起こし](recording-transcription.md) を参照。

## 選定とモデル

- whisper.cpp: ローカルのモデルファイルから多言語音声認識を実行できる。
  Apple Silicon の Metal を利用でき、CPU でも動作する。
- 初期検証モデル: **Whisper base 多言語版**、約 148 MB、MIT ライセンス。
  `.en` モデルは日本語に使えないため API で拒否する。
- base は小さい検証用の出発点。自然発話の精度・遅延・メモリ評価を踏まえ、
  将来 small / medium なども比較する。今回の短い音声の成功から日常入力の精度は保証しない。
- Whisper / whisper.cpp は MIT、whisper-rs は Unlicense。
  モデルのライセンスは [OpenAI Whisper LICENSE](https://github.com/openai/whisper/blob/main/LICENSE)、
  配布元は [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp)。
  アプリに同梱して配布する場合は各ライセンス表示も同梱する。

モデルの準備は明示的な別コマンドに限定する。音声を扱う推論経路はダウンロードや
外部 API を呼ばず、モデル未配置・破損時はローカルエラーを返す。
`prepare-transcription-model.py` は配布元のコミットを固定し、ダウンロード後に SHA-256 を照合してから
指定ファイルを置き換える。音声や認識結果を読み取らない。アプリの明示的な取得操作も[モデル準備](model-setup.md)から利用できます。バックグラウンドでの自動取得は行いません。

- モデル: `ggml-base.bin`
- 配布リビジョン: `5359861c739e955e79d9a303bcbc70fb988958b1`
- SHA-256: `60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe`
- 例として `models/` に配置する（Git 管理対象外）。別のローカルパスも指定できる。

## 実行

Python 3.9 以降、C/C++ コンパイラー、CMake、libclang が必要。Mac では Xcode Command Line Tools と
`brew install cmake` を使う。音声エンジン単体は CI の macOS 26でも検証する。アプリの動作確認は macOS 27で行う。
アプリの対象下限は macOS 27。macOS 26以下はサポート対象外。
Linux では例として `build-essential cmake clang libclang-dev` が必要。

```sh
# 準備時のみネットワークを使う。既存モデルもチェックサムを検証する。
python3 scripts/prepare-transcription-model.py models/ggml-base.bin
cargo test --manifest-path crates/local-transcription/Cargo.toml --locked
cargo build --manifest-path crates/local-transcription/Cargo.toml --release --locked

# ここからはネットワーク不要。macOS の既定は Metal、他 OS は CPU。
crates/local-transcription/target/release/local-transcription \
  models/ggml-base.bin crates/local-transcription/tests/fixtures/ja-oku.wav
# Mac で CPU を明示的に選ぶ場合は末尾に --cpu
```

標準出力はテキスト、日本語の言語指定、選択バックエンド、音声秒数、モデルロード時間、
推論時間を含む JSON。低レベルエンジンの診断は標準エラーに出る。
`backend` は要求した実行方式を示す。whisper.cpp 側の GPU 初期化結果は診断ログを参照する。
CLI は明示的に渡したファイルだけを読み、出力先ファイルを作らない。

## API と次の UI 接続

`Transcriber::load(local_path, Backend)` でローカルモデルをロードし、
`transcribe(&[f32])` で日本語文字列を返す。翻訳・前回の認識文脈は無効。
処理は同期なので UI 接続時はワーカースレッドで実行し、同時実行・破棄・処理中状態を管理する。

入力契約は **16 kHz・モノラル・正規化 f32 PCM、1〜60 秒**。
非有限値、範囲外振幅、空・短すぎる音声、上限超過はエラー。
無音に近い PCM（RMS < 0.00001）は空文字にし、無音への不要なデコードを避ける。
これは VAD ではなく、背景雑音や非発話に対する誤認識対策は後続の検討対象。
CLI の WAV は 16 bit PCM のみ受け付け、不正形式・サンプルレート・チャネル数を拒否する。

現在の録音バッファはデバイスのサンプルレートのまま保持している。
`prepare_audio` で帯域制限を伴う 16 kHz へのリサンプリングを行い、録音停止から
ワーカー経由で呼び出す。モデル配置設定、処理中・結果・エラー・コピー UI も実装済み。
音声は引き続き Rust のメモリ内で扱い、WebView に音声データを渡さない。
エンジン自体にマイク・アクセシビリティ・画面収録の追加許可は不要。

## CI の検証資料

既存の macOS アプリ・実際のマイク許可/拒否 UI のジョブは継続する。
追加の `Offline Japanese ASR (macOS)` ジョブは次を行う。

1. Rust の fmt / Clippy / 単体テスト、Metal 対応バイナリのビルド。
2. 固定したモデルを準備し、SHA-256 を照合。
3. `sandbox-exec` の `(deny network*)` で推論プロセスツリーのネットワークを拒否。
   ローカル TCP 接続も `EPERM` になることを別プローブで確認する。
4. CPU の API テスト、Metal / CPU の CLI で日本語の主要語句、無音、不正 WAV、
   不正レート、モデル未配置・破損を検証。診断ログから Metal 初期化の有無も検証し、
   GPU を要求したのに CPU へフォールバックした場合は CI を失敗させる。結果と実測時間を保存。

Artifacts の `local-transcription-1`（末尾は再実行回数）に JSON とログを保存する。
音声・モデルはアップロードしない。音声の出典と利用条件は
[fixture の説明](../crates/local-transcription/tests/fixtures/README.md) を参照。
この段階で人による権限承認は必要ない。物理マイクの音質・自然発話の精度は後続の評価対象。

CI の GPU は Apple Paravirtual device。Metal の初期コンパイルや仮想 GPU の性能が
計測値に含まれるため、CPU との速度差を実機全体の性能比較には使わない。
