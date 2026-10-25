# 依存更新と48時間の待機期間

JavaScriptはpnpm 12、ビルド用Node.jsは`.nvmrc`の26.12.1を使います。`packageManager`でpnpmを固定し、`pnpm-lock.yaml`と5つの`Cargo.lock`をコミットします。Node.jsはビルド時だけ必要で、配布アプリのmacOS 14以降という条件は変わりません。

## JavaScript

`pnpm-workspace.yaml`の`minimumReleaseAge: 2880`で、直接・間接依存に公開後48時間の待機期間を設定します。`minimumReleaseAgeStrict: true`と`minimumReleaseAgeIgnoreMissingTime: false`により、新しい版しか選べない場合や公開日時を取得できない場合は導入を失敗させます。除外パッケージはありません。依存のビルドスクリプトも未承認では実行させません。

```sh
pnpm install --frozen-lockfile
pnpm run check:dependencies
pnpm run build
```

新しい依存へ更新するときは`pnpm update --latest`を使えます。メジャー更新も対象ですが、48時間の制約は維持します。ロック済み依存もpnpmのポリシー検証を受けます。pnpm本体と、そのネイティブ版を起動するプラットフォーム別パッケージは、実行前にPythonの検査でも公開日時を確認します。

## Rust

安定版Cargoにはpnpm相当の公開後待機設定がないため、`scripts/check-dependency-age.py`でcrates.io sparse indexの公開日時とチェックサムを検査します。アプリ・音声認識・補正・モデル準備・比較計画の5つのロックファイルについて、実行プラットフォームで使われないものも含め、全レジストリ依存を検査します。公開後48時間未満、公開日時不明、チェックサム不一致、未対応の外部レジストリ／Git依存があれば停止します。リポジトリ内のpath依存には公開日時がないため、この公開待機検査の対象にしません。

通常CIはこの検査が成功してからCargoのコンパイルを開始します。Releaseとモデル評価のワークフロー、通常のアプリ用ランタイム準備にも同じ検査を組み込みます。公開メタデータのキャッシュは不変の公開日時を再利用し、ロックファイルに新しい版があれば取得し直します。依存のコードやbuild.rsは、公開日時の検査では実行しません。

手動の`cargo update`自体には待機期間の制約はありません。更新後、**コンパイルより先に**次を実行してください。

```sh
python3 scripts/check-dependency-age.py
```

検査で新しすぎる版が見つかったら、48時間が経過するまで取り込みを待つか、対象のロックファイルに対して`cargo update --manifest-path <Cargo.toml> -p <crate>@<version> --precise <適格版>`で直近の互換版に固定します。再検査が成功してから、`--locked`でビルド・Clippy・テストを実行してください。メジャー更新は依存指定と利用APIも合わせて変更します。

Renovateを実行する場合も、`renovate.json`の`minimumReleaseAge: "2 days"`と`timestamp-required`を適用します。この設定ファイルの追加だけではRenovateサービスは有効になりません。Renovateが更新する場合でも、間接依存を含む最終結果はCIで検査します。

## 今回の更新

公開後48時間を経過した安定版を調査し、TypeScript 6→7、pnpm 12、Node.js 26、rubato 0.16→5、sha2 0.10→0.11へ更新しました。Tauri・Whisper・llama.cppバインディングなどは公開済みの最新安定版を維持し、その他の直接依存の下限も調査時点の適格版へ更新しています。間接依存は親パッケージが要求する互換範囲内の最新版を選びます。

更新時点で48時間未満だったbrotli 9.1.0、brotli-decompressor 6.2.0、erased-serde 0.4.11、smallvec 1.16.4などは取り込まず、待機条件を満たす互換版を固定しています。GitHub Actionsも公開後2日以上経過した現行リリースへ更新し、可変のメジャータグではなくコミットSHAで固定しています。

`v0.0.1`も初回Releaseに使えます。タグに合わせてアプリのバージョンをCIが設定します。既に`0.1.0`などのアプリを使用している場合、自動更新では`0.0.1`へ下げません。初回のDMGから手動でインストールしてください。
