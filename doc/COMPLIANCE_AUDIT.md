# Meta Platform 準拠監査 (COMPLIANCE_AUDIT)

監査日: 2026-07-23
対象: instagram-crm (Rust / Tauri 2 デスクトップ常駐アプリ) — コミット `54c36ac` 時点の自作コード

## 結論

**対象コードの静的レビュー範囲では、非公式API・ブラウザ自動化・スクレイピング・
認証情報の不正利用は確認されなかった。**
自作コードが行う外向き通信は Meta 公式の Instagram Platform API (`graph.instagram.com`) のみ。

### 監査の方法と限界

- 方法: 自作コード (`src-tauri/src/`, `ui/`) の目視レビュー + 禁止事項キーワードの文字列スキャン (後述)
- 限界:
  - 依存クレート (Cargo.lock 記載) の内部実装までは監査していない
  - 実行時の外向き通信のパケットキャプチャ等による動的検証は行っていない
  - 本監査以降のコード変更は対象外 (変更時は再確認が必要)

## 現状の構成

| 項目 | 現状 |
|---|---|
| 認証経路 | Meta App Dashboard で手動発行した長期アクセストークンをUIから貼り付け → `GET /me` で検証 → macOS Keychain / Windows Credential Manager へ保存 (`keyring` crate)。App Secret はアプリに存在しない |
| トークン維持 | `GET /refresh_access_token` (grant_type=ig_refresh_token) で60日トークンを自動更新 |
| コメント取得経路 | ポーリング (既定30秒)。`GET /me/media` → `media_product_type == "REELS"` で絞り込み → `GET /{media-id}/comments` |
| 返信経路 | `POST /{comment-id}/replies` (message) — 公式エンドポイント |
| 使用権限 | `instagram_business_basic`, `instagram_business_manage_comments` (トークン発行時に付与。投稿公開・メッセージ権限は不要のため未使用) |
| Graph APIバージョン | v25.0 (`src-tauri/src/api/instagram.rs` にハードコード → 今回設定化) |
| Webhook | 未使用 (設計書の方針: サーバーなし・Webhookなし・Pull方式) |
| 重複送信防止 | SQLite `comments` テーブル (comment_id PRIMARY KEY) + 送信前の `is_replied` チェック + `INSERT OR IGNORE` |
| リトライ | HTTP 429 で指数バックオフ (interval×1→2→4→8倍、成功で復帰)。その他APIエラーはリトライせず次周期 |
| ログ | トークン実値・Cookie は出力なし (エラーメッセージのみ抽出)。コメント本文・ユーザー名は APIから取得すらしない (fields=id,timestamp,from) |

## 禁止事項スキャン結果

自作コード全体への `grep -ri` による文字列スキャン (依存クレートの実装は対象外):

`selenium` / `playwright` / `puppeteer` / `instagrapi` / `instagram_private_api` /
`sessionid` / `csrftoken` / `instagram.com/api/v1` / `graphql/query` / `query_hash` /
`proxy rotation` / `captcha` / `device fingerprint` — **上記キーワードはいずれも検出なし**
(キーワードに現れない手法まで否定するものではない)。

`password` の一致は keyring crate のAPI名 (`set_password`/`get_password` = OSキーチェーン操作) と
トークン入力欄のマスク表示 (`type="password"`) のみで、Instagramパスワードの取り扱いは存在しない。

## 規約上の高リスク箇所と修正結果 (全て実装済み)

| # | リスク | 修正 (実装済み) |
|---|---|---|
| 1 | **全コメントへ無差別に自動返信する**(スパム的挙動、Platform Policyの automation 品質要件に抵触し得る) | 対象フィルタ追加 (許可media_id・キーワード・最大経過時間・最大本文長・空文字除外)。**ドライラン既定ON** |
| 2 | 返信成功後の DB 書き込み失敗時、次周期に再送 → 二重返信の可能性 | 送信「前」に processing として確保し、結果を succeeded/failed/unknown/queued で記録。タイムアウト等の **unknown は自動再送しない** (手動照合: RUNBOOK)。クラッシュ時のprocessing残留は起動時にunknownへ移行 |
| 3 | APIバージョンのハードコード | 設定 `meta_graph_api_version` へ外出し (形式バリデーション付き、"latest"不可) |
| 4 | 緊急停止手段がない | kill switch (UIトグル + settings) を追加。API使用量 (x-app-usage) が閾値超過時も送信を自動停止 |
| 5 | Rate Limit ヘッダー未参照 | `Retry-After` を待機時間の下限として尊重。指数バックオフに full jitter 追加。恒久エラー (権限不足・無効ID・失効トークン) は自動再試行しない |

## 方針決定事項

- **Webhookは採用しない**: Meta comments Webhookは公開HTTPSサーバーが必須であり、
  本アプリの「サーバーなし」設計 (design_doc) と両立しない。ポーリングは公式APIの正規の利用方法であり規約違反ではない (ユーザー承認済み 2026-07-23)
- **Private Reply (DM) は実装しない**: MVP範囲外。メッセージ系権限も要求しない (ユーザー承認済み 2026-07-23)

## 手作業が必要な Meta App Dashboard 設定

META_SETUP.md を参照。要点: Instagramプロアカウント、Metaアプリ作成、
「Instagram API with Instagram Login」セットアップ、Token Generator での長期トークン発行、
テスターのロール設定、(第三者配布時のみ) App Review。

## 修正後も残るリスク

- ポーリング方式のため、Webhook比で返信遅延がありAPI消費が多い (Rate Limit `4800×インプレッション/24h` の範囲内で運用)
- トークン発行日時をAPIから取得できないため、貼り付け時点+60日と仮定している (失効時は再連携導線あり)
- 開発モードのMetaアプリではテスター登録済みアカウントのみ動作。第三者提供には App Review (Advanced Access) が必要
- unknown 状態の返信は手動確認が必要 (OPERATIONS_RUNBOOK.md 参照)
