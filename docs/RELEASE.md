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

### macOSコード署名・公証 (Apple Developer Program必須)

未署名だとmacOSで「壊れているため開けません」と表示されるため、以下のSecretsも登録する。

| Secret名 | 値 |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application証明書 (.p12) をbase64化した文字列 |
| `APPLE_CERTIFICATE_PASSWORD` | .p12エクスポート時に設定したパスワード |
| `APPLE_SIGNING_IDENTITY` | 証明書名 (例: `Developer ID Application: 氏名 (TEAMID)`) |
| `APPLE_ID` | Apple IDのメールアドレス |
| `APPLE_PASSWORD` | App用パスワード (https://account.apple.com で生成) |
| `APPLE_TEAM_ID` | Team ID (Developer Portal → Membership) |

証明書の作成〜登録手順:

1. [Apple Developer Portal](https://developer.apple.com/account/resources/certificates/list) で **Developer ID Application** 証明書を作成 (CSRはキーチェーンアクセス → 証明書アシスタントで生成)
2. ダウンロードした証明書をキーチェーンに取り込み、秘密鍵ごと `.p12` 形式で書き出す (パスワードを設定)
3. base64化してSecretsに登録:

   ```sh
   base64 -i certificate.p12 | pbcopy
   ```

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
