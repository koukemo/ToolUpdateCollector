# tuc — Tool Update Collector

`apt` / `brew` / `claude` などの更新コマンドを登録しておき、TUIからまとめて実行するツール。

## インストール

[Releases](https://github.com/koukemo/ToolUpdateCollector/releases) から取得する。

### Linux（Debian / Ubuntu）

```sh
sudo apt install ./tuc_<version>-1_amd64.deb
```

### Windows

`tuc_<version>_windows_x86_64.zip` を展開し、`tuc.exe` を PATH の通ったフォルダ（例: `%USERPROFILE%\.local\bin`）に置く。

```powershell
tar -xf tuc_<version>_windows_x86_64.zip -C $env:USERPROFILE\.local\bin
```

`tuc.exe` は署名していないため、初回の起動時に SmartScreen の警告が出ることがある。

## 使い方

```sh
tuc              # TUIを起動
sudo tuc         # root で起動。sudo のパスワード入力なしで全コマンドを実行
tuc run          # TUIを開かずに実行
tuc list         # 一覧表示
tuc add "brew update && brew upgrade" -n brew
tuc update       # 最新版に更新 (--check で確認のみ)
tuc config-path  # 設定ファイルの場所
tuc --config <path> ...  # 設定ファイルを指定 (環境変数 TUC_CONFIG でも可)
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

### Windows の管理者権限

Windows 版では「root権限が必要」が **管理者権限が必要** になる（`sudo` を含むと自動でオンになるのは同じ）。

- **管理者として実行したターミナル**で `tuc` を起動すると、タイトルに `[管理者: 対話なしで実行]` と出て、全コマンドを管理者権限で実行する。
- 通常のターミナルで起動して、管理者権限が必要なコマンドが有効になっていると、実行時に UAC の確認が1回出る。許可すると、管理者として起動した別ウィンドウの tuc が全コマンドを実行する。終わると結果を表示して Enter 待ちになり、元のターミナルには終了コードが返る。UAC を拒否すると、何も実行せずに中止する。
- 「WSLで実行」のコマンドは Windows の管理者権限とは関係なく、WSL の既定ユーザーで実行する（WSL 内の `sudo` は WSL 側のパスワードを求める）。
- `tuc update` は `tuc.exe` を置き換える。`Program Files` など書き込みに管理者権限が要る場所に置いた場合は、管理者として実行したターミナルで `tuc update` を実行する。

設定ファイル: `~/.config/tuc/config.toml`（Windows は `%APPDATA%\tuc\config.toml`）

## リリース

`v*` タグをpushすると、GitHub Actionsが `.deb`（Linux）と `tuc_<version>_windows_x86_64.zip`（Windows）をビルドしてReleaseに添付する。

## ライセンス

[MIT License](LICENSE)

依存しているクレートのライセンスは [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt) にまとめ、`.deb`（`/usr/share/doc/tuc/`）と Windows 版の zip にも同梱している。依存を変えたら、次のコマンドで作り直す:

```sh
cargo install cargo-about --locked --features cli
cargo about generate about.hbs -o THIRD_PARTY_LICENSES.txt
```
