---
name: release
description: tuc（このリポジトリ）の変更をコミットしてPRを作り、rebase mergeでmainに取り込み、バージョンタグを打ってGitHub Releaseに.debを配置する。ユーザーが /release を実行したとき、または「リリースして」「PR作ってマージしてリリース」と頼んだときに使う。引数はバージョン指定（patch / minor / major / X.Y.Z）、省略時は patch。
---

# /release（tuc 用）

commit → PR → rebase merge → タグ → GitHub Actions で `.deb` をビルド → Release に添付、までを行う。
/release の実行そのものが、コミット・push・PR作成・マージ・タグ作成の許可になる。ただし「止まる」と書いた条件に当たったら、ユーザーに確認してから進める。

前提:
- リポジトリは `koukemo/ToolUpdateCollector`、既定ブランチは `main`。
- リリースのworkflowは `.github/workflows/release.yml`。`v*` タグのpushで `cargo deb` を実行し、`target/debian/*.deb` を Release に添付する。
- `tuc update` は最新 Release の `*_amd64.deb` / `*_arm64.deb` を探す。なので、**成果物のファイル名は `tuc_<ver>-1_<arch>.deb` 形式を崩さない**。

## 0. 事前確認

```bash
git status --short
git branch --show-current
git remote -v
gh auth status
gh repo view --json nameWithOwner,visibility,defaultBranchRef
```

- `gh` が未ログイン → 止まって伝える。
- GitHub にリポジトリがない → 公開か非公開かを確認してから、`gh repo create koukemo/ToolUpdateCollector --<public|private> --source . --remote origin` で作る。
  - 非公開だと、`tuc update` が認証なしで Release を取得できない。確認するときに、この点も伝える。
- コミットに入る差分（未追跡ファイルも含む）に、秘密鍵、トークン、`.env`、パスワードらしき文字列がないかを確認する。

## 1. ローカルでの検証

```bash
cargo fmt --check
cargo clippy --release -- -D warnings
cargo build --release
cargo deb
```

- 失敗したら直してから進める。直した内容もコミットに含める。
- 実際の更新コマンド（`tuc run` や、TUIでの実行）は**動かさない**。ユーザーの環境のパッケージが実際に更新されてしまう。動作確認には `TUC_CONFIG=<scratchpad>/x.toml` を指定し、`echo` などの無害なコマンドだけを使う。

## 2. バージョンを決める

- 現在の版は `Cargo.toml` の `[package] version` から読む。
- 引数（patch / minor / major / `X.Y.Z`）から次の版を決める。省略時は patch。
  - 初回のリリース（まだタグがないとき）で、引数を省略した場合は、`Cargo.toml` の版をそのまま使う。
- `vX.Y.Z` のタグがすでにある（`git tag -l` / `git ls-remote --tags origin`）→ 止まって確認する。
- `Cargo.toml` の version を書き換えてから `cargo update -p tuc --offline`（失敗したら `cargo build`）を実行し、`Cargo.lock` も合わせる。

## 3. ブランチ・コミット・PR

- `main` にまだ1つもコミットがない場合: `.gitignore` だけの initial commit を `main` に作って push する（PRのベースにするため）。
- いまのブランチが `main` なら、`release/vX.Y.Z` を切る。それ以外のブランチなら、そのまま使う。
- 変更をまとまりごとにコミットする（例: 機能の変更 / CI / バージョン更新 `chore: release vX.Y.Z`）。メッセージは日本語でも英語でもよい。末尾にはattributionを付ける。
- `git push -u origin <branch>`
- `gh pr create --base main --title "Release vX.Y.Z" --body ...`
  - 本文には、変更の要約、成果物（`tuc_X.Y.Z-1_amd64.deb`）、ローカルで検証した内容を書く。

## 4. rebase merge

```bash
gh pr checks <PR> --watch        # PR向けのCIがなければ、すぐに戻る
gh pr merge <PR> --rebase --delete-branch
```

- チェックが失敗 → マージしない。直せるものは直して push し直す。
- リポジトリの設定で rebase merge が無効 → 止まって伝える。他の方式で勝手にマージしない。
- コンフリクト → `git fetch origin && git rebase origin/main` で解消して push する。解消に判断が要るときは確認する。

## 5. タグ → リリース

```bash
git switch main && git pull --ff-only
grep -m1 '^version' Cargo.toml   # タグと一致しているかを確認
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
gh run list --workflow release.yml --limit 1
gh run watch <run-id> --exit-status
```

- 失敗したら `gh run view <id> --log-failed` で原因を見る。直したら、修正を同じ手順（PR → rebase merge）で取り込む。
- まだ成果物のない、または draft のリリースなら、タグを付け直してよい: `gh release delete vX.Y.Z -y; git push --delete origin vX.Y.Z; git tag -d vX.Y.Z` してから付け直す。
- 成果物つきで公開済みのリリースのタグは動かさず、patch を上げて出し直す。

## 6. 確認と報告

```bash
gh release view vX.Y.Z --json url,isDraft,assets --jq '{url, isDraft, assets: [.assets[].name]}'
cargo build --release -q && ./target/release/tuc update --check   # 公開リポジトリのみ
```

- `tuc_X.Y.Z-1_amd64.deb` が添付され、`isDraft: false` になっていることを確認する。
- `tuc update --check` で新しい版が見えることを確認する。ローカルの版と同じなら「最新版です」と出るのが正しい。
- 報告すること: PRのURL、マージしたcommit、リリースのURL、成果物の一覧。失敗したことや、やっていないことがあれば、それもはっきり書く。
