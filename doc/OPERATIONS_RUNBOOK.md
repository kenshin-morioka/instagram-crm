# 運用手順書 (OPERATIONS_RUNBOOK)

ログ: `~/Library/Logs/com.kenshinmorioka.instagram-crm/instagram-crm.log`
DB: `~/Library/Application Support/com.kenshinmorioka.instagram-crm/app.db`

## kill switch (緊急停止)

- 停止: アプリ画面の **「送信を一時停止」** を押す (即時。以降の周期は送信・取得ともスキップ)
- 再開: 同じボタン (「送信を再開」)
- UIが操作できない場合: `sqlite3 app.db "UPDATE settings SET value='true' WHERE key='sending_paused';"` → アプリ再起動不要 (次周期から反映)
- 最終手段: トレイの Quit でアプリ自体を終了

## HTTP 429 (Rate Limit) が出た場合

- 自動対応: Retry-After尊重 + 指数バックオフ (最大で通常間隔の8倍) で自動回復する
- 頻発する場合: `polling_interval_secs` を延ばす / `media_fetch_limit` を減らす
- API使用量が `usage_pause_threshold_pct` (既定90%) を超えると送信は自動停止する

## トークン失効時

- 症状: ログに「OAuth期限切れ」、画面に「トークンの再設定が必要です」
- 対応: META_SETUP.md の手順でトークンを再発行し、貼り付けて再連携

## 権限・App Reviewエラー時

- 症状: ログに `コメント返信失敗` + Meta error code (10, 200番台等)。該当コメントは `failed` として記録され自動再試行しない
- 対応: トークンのスコープに `instagram_business_manage_comments` が含まれるか確認し、
  含まれていなければ正しいスコープで再発行。テスター登録も確認

## 重複返信が発生した場合

1. まずkill switchで送信を停止
2. `sqlite3 app.db "SELECT comment_id, status, attempt_count, reply_id FROM comments ORDER BY started_at DESC LIMIT 50;"` で状況確認
3. Instagram上で余分な返信を手動削除
4. 原因 (unknown後の手動再実行等) を特定してから再開

## 送信結果不明 (unknown) の対応

- 発生条件: 送信後のタイムアウト、または送信中のクラッシュ。**自動再送はしない**
- 確認: `sqlite3 app.db "SELECT comment_id, media_id, started_at FROM comments WHERE status='unknown';"`
- Instagram上で該当コメントに返信が付いているか目視確認し、
  - 付いていれば: `UPDATE comments SET status='succeeded' WHERE comment_id='...';`
  - 付いていなければ: `UPDATE comments SET status='queued' WHERE comment_id='...';` (次周期で再送される)

## Meta側の一時制限 (アカウント/アプリ制限) が発生した場合

- 送信をkill switchで停止し、App Dashboardのアラートを確認する
- 制限を別アプリ・別アカウントで迂回しない (規約違反)
- 制限解除後、`dry_run: true` で挙動を確認してから再開する

## 再試行待ち (queued) の滞留

- `sqlite3 app.db "SELECT COUNT(*) FROM comments WHERE status='queued';"`
- 滞留する場合はログのエラー内容を確認 (一時エラーが続いている)。
  Rate Limitなら間隔を延ばす、それ以外はトークン・権限を確認

## Private Reply (DM) について

- 本アプリはDM機能を実装していない (停止手順は不要)。メッセージ権限も要求していない

## アクセストークン漏えい時

1. kill switchで送信停止
2. Instagramアプリ: 設定 → アプリとウェブサイト → 該当アプリの連携を解除 (トークンが失効する)
3. 必要ならMeta App DashboardでApp Secretをローテーション (Settings → Basic → Reset)
4. 新しいトークンを発行して再連携
5. Keychainの旧エントリはアプリが上書きするため手動削除は不要
