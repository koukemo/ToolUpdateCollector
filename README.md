# tuc — Tool Update Collector

`apt` / `brew` / `claude` などの更新コマンドを登録しておき、TUIからまとめて実行するツール。

## インストール

[Releases](https://github.com/koukemo/ToolUpdateCollector/releases) から `.deb` を取得:

```sh
sudo apt install ./tuc_<version>_amd64.deb
```

## 使い方

```sh
tuc              # TUIを起動
sudo tuc         # root で起動。sudo のパスワード入力なしで全コマンドを実行
tuc run          # TUIを開かずに実行
tuc list         # 一覧表示
tuc add "brew update && brew upgrade" -n brew
tuc update       # 最新版に更新 (--check で確認のみ)
tuc config-path  # 設定ファイルの場所
```

### TUIのキー操作

| キー | 操作 |
|---|---|
| Tab / Shift+Tab | 一覧 → [実行] → [キャンセル] とフォーカス移動 |
| Enter | フォーカスしているボタンを押す / 一覧では編集 |
| ↑↓ / j k | 選択 |
| Space | 有効/無効の切り替え |
| a / e / d | 追加 / 編集 / 削除 |
| J K / Shift+↑↓ | 並べ替え |
| r | すぐに実行 |
| q / Esc | 終了 |

### コマンドごとのオプション

- **対話**: 標準入力をつなぐ。オフのときは標準入力を閉じるので、入力待ちで止まらない
- **最後に実行**: 他のコマンドがすべて終わった後に実行
- **root権限が必要**: `sudo` を含むと自動でオンになる。通常起動のときは、最初に `sudo -v` でまとめて認証する
- **WSLで実行**（Windows版のみ。Linux / macOS 版には存在しない）: `wsl.exe -- bash -lc` 経由で実行する

`sudo tuc` で起動すると、root不要のコマンド（brewなど）は `sudo -i -u $SUDO_USER` で元のユーザーとして実行する。設定ファイルも元のユーザーのものを使う。

設定ファイル: `~/.config/tuc/config.toml`

## リリース

`v*` タグをpushすると、GitHub Actionsが `.deb` をビルドしてReleaseに添付する。

## ライセンス

[MIT License](LICENSE)

依存しているクレートのライセンスは [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt) にまとめ、`.deb` にも同梱している（`/usr/share/doc/tuc/`）。依存を変えたら、次のコマンドで作り直す:

```sh
cargo install cargo-about --locked --features cli
cargo about generate about.hbs -o THIRD_PARTY_LICENSES.txt
```
