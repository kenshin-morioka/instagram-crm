# リリース手順 (RELEASE)

## 事前準備 (初回のみ)

アップデーター署名鍵をGitHubリポジトリのSecrets (Settings → Secrets and variables → Actions) に登録する。

| Secret名 | 値 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | 秘密鍵ファイルの中身 (文字列) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 鍵のパスワード (パスワードなしで生成した場合は空文字) |

- 公開鍵は `src-tauri/tauri.conf.json` の `plugins.updater.pubkey` に埋め込み済み
- **秘密鍵を紛失すると既存ユーザーへ自動アップデートを届けられなくなる** (公開鍵を変えた新バイナリを手動で入れ直してもらうしかない)。安全な場所に保管すること
- 鍵の再生成: `npx @tauri-apps/cli signer generate -w <出力先>` → pubkeyを差し替え

## リリースの流れ

1. `src-tauri/tauri.conf.json` と `src-tauri/Cargo.toml` の `version` を上げてmainにマージ
2. バージョンタグをpushする:

   ```sh
   git tag v1.0.0
   git push origin v1.0.0
   ```

3. GitHub Actions (`.github/workflows/release.yml`) が自動で以下を行う:
   - macOS (Apple Silicon / Intel) の `.dmg` と Windows の `.msi` / `.exe` をビルド
   - 各バイナリに署名 (アップデーター用)
   - GitHub Releaseを作成し、アセットと `latest.json` を添付

## 自動アップデートの仕組み

- アプリは起動時と6時間ごとに `releases/latest/download/latest.json` を確認する
- 新しいバージョンがあるとアプリ上部にバナーが出て、
  「最新バージョンをインストール」ボタンでダウンロード・インストール・再起動する
- `latest.json` の署名を `pubkey` で検証するため、署名されていないバイナリには更新されない
