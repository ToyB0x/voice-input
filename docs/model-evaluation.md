# 日本語モデルの比較

## 範囲

Whisper の多言語版 base / small を、同じ固定音声・同じ認識設定で比較する。
medium は準備スクリプトと手動起動のCIで追加評価できる。
モデルの自動ダウンロードや外部の認識 API はアプリへ追加しない。
モデル名・ファイル容量は、読み込んだモデルに基づいてアプリに表示する。
任意のファイル名に変えても base / small を識別できる。

正解文を持つ FLEURS の日本語読み上げ音声7件（合計約1分）と2秒の無音を固定した。
日常表現、依頼、仕事・商取引に関連する文、数字・固有名詞を含む。
**自由会話・仕事の口述を収録したデータではない**。話者IDは提供されておらず、
異なる gender ラベルの行を含むが、個別の話者数は主張しない。
出典・CC BY 4.0 の帰属・変更点は [fixtureの説明](../evaluation/fixtures/README.md)、
音声と正解文のID・SHA-256は [corpus.json](../evaluation/corpus.json) に記録する。
選定はモデルの推論結果を見る前に行い、認識結果に合わせて正解を変更しない。
元データが学習に含まれていた可能性もあるため、未知データへの精度保証には使わない。

## 測定方法

- 本番と同じ `Transcriber` を呼ぶ専用CLI `evaluate` を使う。正解文は採点だけに使い、モデルへ渡さない。
- 日本語、翻訳なし、前回文脈なし、greedy、スレッド数は利用可能数と4の小さい方。
- 1プロセスでモデルを保持する。ロード時間、最初の音声の初回認識を別枠で記録し、
  その後は各音声を2回測る。同一音声の反復を独立した発話数とは数えない。
- 生の文字列と、NFKC + casefold後にUnicodeの句読点・空白を除いた文字列の両方で
  Levenshtein距離を求める。正規化後CERは `誤り数合計 / 正解文字数合計`。
  文ごとのCERの単純平均にはしない。漢字/かな、漢数字/算用数字などの表記差も誤りとして残る。
- 無音はCERの分母・速度集計から除外し、空でない認識結果を別に失敗として扱う。
- RTFは `認識時間合計 / 音声時間合計`（1未満なら音声の長さより短い時間で処理）。
  各文・各反復の時間、中央値、カテゴリ別のCERと時間も保存する。
- メモリは `wait4` の子プロセス単位の `ru_maxrss`。macOSはbytes、LinuxはKiBからbytesに変換。
  **CLIプロセスの最大RSS**であり、Tauri/WebViewやGPUドライバーのすべての割り当てを含む総RAMではない。
- コードリビジョン・作業ツリーの変更有無・Cargo.lock・実行バイナリのハッシュも保存する。
- 各モデル・各バックエンドは別プロセスで測る。CPUとMetalの実行を同じランナー内で同時に行わない。
  GPUを指定したのにCPUへ切り替わった場合は、診断ログのMetal初期化確認で失敗にする。

## 実行

依存は既存のRustエンジンの開発環境とPython 3.9以降。評価時にpyarrowやffmpegは不要。

```sh
# 準備だけがネットワークを使う。base/small/medium のリビジョン・ハッシュを固定。
python3 scripts/prepare-transcription-model.py --model base models/ggml-base.bin
python3 scripts/prepare-transcription-model.py --model small models/ggml-small.bin
cargo build --manifest-path crates/local-transcription/Cargo.toml --release --locked --bin evaluate
python3 -m unittest discover -s scripts/tests -v

# Macで推論プロセスと採点処理のネットワークを拒否して測る例。
# CPUの代わりに --backend metal でGPUを測る。
sandbox-exec -p '(version 1) (allow default) (deny network*)' \
  python3 scripts/evaluate_models.py \
    --binary crates/local-transcription/target/release/evaluate \
    --model models/ggml-small.bin --model-name small --backend cpu \
    --output evaluation/results/small/cpu --require-network-denied
```

Linuxの開発環境では `sandbox-exec` を使わず、ネットワークのないコンテナなどでCPUを測れる。
`--require-network-denied` はmacOSの拒否ポリシーによるEPERMを確認するフラグなので、
ネットワークのないLinuxコンテナでの実行には付けない。その結果をCIの拒否検証済みと混同しない。
`evaluation/results/` はGit対象外。音声・モデルは結果にコピーしない。

base/smallに加えmediumを測る場合は `--model medium` で準備する。
モデルの固定情報は [transcription_models.py](../scripts/transcription_models.py) に集約している。

## CIと比較結果

`Model evaluation` は週次・手動実行で base と small を、それぞれCPU・Metalで評価する。通常のPR / main pushでは全件比較を起動しない。[CIの実行範囲](ci.md)を参照。
手動実行の `include_medium` を有効にするとmediumも追加する。
各プロセスツリーは `sandbox-exec (deny network*)` の中で実行し、
localhostへのTCP接続がEPERMになることもその場で確認する。

全モデル×全バックエンドのレポートが揃い、同じコーパス・コードリビジョン・採点プロトコル・Cargo.lockと固定モデルのハッシュである場合だけ
比較表と暫定のCPU順位を生成する。欠落・重複・別のコーパス・通信拒否の未検証はエラーとする。
この小規模評価の順位を、実機GPUの速度比較や一般的な認識精度の順位には置き換えない。
標準候補の選定は、実際の比較表と制約を併記して行う。

Artifacts（14日）:

- `model-evaluation-base` / `model-evaluation-small`: `cpu/`・`metal/` に `report.json`、
  全文の正解・認識結果・CER・時間を含む `cases.csv`、エンジン診断。環境情報・重み・コーパスのハッシュも記録。
- `model-evaluation-summary`: `summary.md`、全比較の `comparison.json`、暫定CPU順位の `recommendations.json`。
- 音声・モデルはアップロードせず、出典と利用条件を添える。GitHub ActionsのSummaryにも比較表を掲載する。

既存のアプリCIでは、名前を `local-model.bin` にしたbaseモデルを読み込み、
画面の情報が「base / 148 MB」となることを確認する。マイク許可・拒否、録音、認識、コピー、破棄も継続する。
モデル比較のための追加OS権限はない。権限の確認を人手へ移す必要はない。

自由会話・雑音・物理マイクでの精度、実機GPUの速度とメモリは、この固定コーパスの評価とは別に確認する。

## 初回の実測と暫定候補

[Mac CIの測定実行](https://github.com/ToyB0x/voice-input/actions/runs/40042105375) の
[全レポート・生成比較表](../evaluation/baseline/README.md) をリポジトリに保存した。
Apple M1 (Virtual)、3論理CPU、7 GiB RAM、macOS 14で、発話61.14秒＋無音2秒を使用した。
各モデル内のCPU→Metalは同じランナー、モデル間は同一仕様の別ランナーである。

| モデル | バックエンド | CER | RTF | 認識時間中央値 | 最大RSS |
| --- | --- | ---: | ---: | ---: | ---: |
| base | CPU | 34.98% | 0.149 | 1.280秒 | 443.7 MiB |
| base | Metal | 34.98% | 0.470 | 3.904秒 | 549.1 MiB |
| small | CPU | 15.27% | 0.453 | 3.962秒 | 878.0 MiB |
| small | Metal | 15.27% | 1.134 | 10.481秒 | 1036.8 MiB |

この読み上げ音声では、**精度を優先する暫定候補はsmall、速度・メモリを優先する候補はbase**。
CPUでsmallのCERは約19.7ポイント低いが、RTFは約3倍、最大RSSは約2倍となった。
smallは「旅行代理店」「公開サイトに掲載」などを正しく認識したが、
「変更願い」「柏崎刈羽原発」は両モデルとも誤変換した。
この結果だけで自由な口述の実用精度を保証しない。
全モデル・バックエンドで無音の誤出力は0件だった。

モデルは利用者が明示的に準備・指定する方式を維持する。精度重視で試す場合は
`python3 scripts/prepare-transcription-model.py --model small models/ggml-small.bin`
で準備したファイルを指定する（約488 MB）。baseは約148 MB。
mediumは今回の標準候補にせず、必要に応じて手動CIで追加比較する。

CPUとMetalでCERは同じだった。**GPU化による精度向上は確認されていない**。
この仮想GPUではMetalが遅かったため、実機MacでのGPU高速化の有無・倍率や
バックエンドの推奨変更には使わない。アプリでは引き続きMetal / CPUを選択できる。
モデルロード時間はOSのファイルキャッシュなどの影響を受け、CPU→Metalの順で実行したため、
ロード時間の差もバックエンドの公平な比較には使わない。
