# Instagram CRM

自分のInstagramリールに付いたコメントへ、公式Graph APIで定型文を自動返信するローカル常駐アプリ (Rust / Tauri 2)。

- Meta公式API (`graph.instagram.com`) のみ使用。スクレイピング・非公式API・ブラウザ自動化は不使用
- サーバーレス (ポーリング方式、既定30秒)
- トークンはOSのKeychain / Credential Managerに保存
- **既定はドライラン** (返信対象をログに出すだけで送信しない)
- **対応OS: macOS / Windows のみ** (トークン保存が両OSのネイティブ機構前提のため、Linuxでは動作しない)

## セットアップ

1. Meta側の準備は [doc/META_SETUP.md](doc/META_SETUP.md) を参照 (プロアカウント・Metaアプリ・長期トークン発行)
2. ビルドと起動 (開発時):

   ```sh
   cd src-tauri
   cargo run
   ```

   配布・日常利用にはmacOSアプリとしてビルドする (ターミナルなしで起動できる):

   ```sh
   npx @tauri-apps/cli build
   # → src-tauri/target/release/bundle/macos/Instagram CRM.app
   # アプリケーションフォルダへコピーして使う
   ```

3. アプリの「接続状態」欄に長期アクセストークンを貼り付けて「連携する」
4. ドライランのまま数周期動かし、ログの `[DRY RUN]` で対象判定を確認
5. 問題なければアプリUIの「ドライランを解除」ボタンを押す (実送信が始まる)

## 設定ファイル

場所: `~/Library/Application Support/com.kenshinmorioka.instagram-crm/config.json` (macOS)。
初回起動時に自動生成される。全項目は [config.example.json](config.example.json) を参照。

| キー | 既定 | 説明 |
|---|---|---|
| `meta_graph_api_version` | v25.0 | Graph APIバージョン |
| `dry_run` | **true** | 初期値。以降はアプリUIの「ドライランを解除」ボタンで切り替える。解除後、`comment_lookback_hours` 内に検出済みのコメントには返信される |
| `allowed_media_ids` | [] | 返信対象リールの限定 (空=全リール) |
| `reply_keywords` | [] | 本文にいずれかを含む場合のみ返信 (空=全件) |
| `comment_lookback_hours` | 24 | これより古いコメントは対象外 |
| `max_comment_length` | 500 | これより長いコメントは対象外 |
| `usage_pause_threshold_pct` | 90 | API使用量がこの%を超えたら送信を一時停止 |

返信文とポーリング間隔はアプリのUIから変更する。

## 運用

障害対応・kill switch・トークン失効時の手順は [doc/OPERATIONS_RUNBOOK.md](doc/OPERATIONS_RUNBOOK.md) を参照。
規約準拠の監査結果は [doc/COMPLIANCE_AUDIT.md](doc/COMPLIANCE_AUDIT.md) を参照。

## テスト

```sh
cd src-tauri
cargo test
```

実APIへの送信は行わない (全てローカルのユニットテスト)。

## ログ

- macOS: `~/Library/Logs/com.kenshinmorioka.instagram-crm/instagram-crm.log`
- トークン・コメント本文・ユーザー名はログに出力しない
