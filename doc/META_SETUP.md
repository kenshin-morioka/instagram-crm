# Meta App Dashboard セットアップ手順 (META_SETUP)

このアプリは「Instagram API with Instagram Login」を使用する。Facebookページは不要。

## 1. 前提: Instagramプロアカウント

- 対象アカウントを**プロアカウント (ビジネス or クリエイター)** に切り替える
- Instagramアプリ: 設定 → アカウントの種類とツール → プロアカウントに切り替える
- 個人アカウントのままではAPIを使用できない

## 2. Metaアプリの作成

1. https://developers.facebook.com/ → My Apps → Create App
2. ユースケースで **Instagram** を選択 (Instagram API with Instagram Login)
3. 作成後、左メニュー「Instagram > API setup with Instagram login」を開く

## 3. 認証方式

- 本アプリは `instagram_login` 方式のみ対応 (config.jsonの `meta_auth_mode`)
- Facebook Login方式 (Facebookページ紐付け・Page Access Token) は未実装

## 4. 必要な権限 (スコープ)

トークン発行時に以下のみ付与する。それ以外 (公開投稿・メッセージ権限等) は要求しない。

- `instagram_business_basic` (必須。トークンリフレッシュにも必要)
- `instagram_business_manage_comments` (コメント取得・返信)

## 5. アクセストークンの発行

1. 「API setup with Instagram login」内の **Token Generator** を開く
2. 対象のプロアカウントを追加 (App Roles → Instagram Testers に追加し、
   Instagram側の 設定 → アプリとウェブサイト → テスター招待 で承認)
3. 上記2スコープを選択して Generate Token
4. 表示された**長期トークン (60日有効)** をアプリの「接続状態」欄へ貼り付け

## 6. トークンの更新・失効

- 更新: アプリが `GET /refresh_access_token` で自動リフレッシュする (期限7日前から試行)。
  60日以上アプリを起動しないと失効するため、その場合は再発行して貼り直す
- 失効させたい場合: Instagramアプリの 設定 → アプリとウェブサイト から連携を解除するか、
  Meta App Dashboard でアプリを無効化する (OPERATIONS_RUNBOOK.md 参照)

## 7. Standard / Advanced Access と App Review

- **開発モード + Standard Access** のままで、Instagram Testers に登録した自分のアカウントに対して全機能が動作する (自分用途はここまでで完結)
- 不特定の第三者のアカウントで使わせる場合のみ、App Review で
  `instagram_business_manage_comments` の **Advanced Access** を申請する
- 確認箇所: App Dashboard → App Review → Permissions and Features

## 8. Webhookについて

本アプリはポーリング方式でWebhookを使用しない (サーバーレス設計のため)。
Webhook callback URL・verify token・commentsフィールド購読の設定は不要。

## 9. 本番運用前チェックリスト

- [ ] 対象アカウントがプロアカウントである
- [ ] トークンのスコープが上記2つのみである
- [ ] ドライラン (既定ON) のまま数周期動かし、`[DRY RUN]` ログで対象判定が意図どおりか確認した
- [ ] `allowed_media_ids` / `reply_keywords` を必要に応じて設定した
- [ ] 返信文が固定テンプレートとして適切 (スパム的でない) ことを確認した
- [ ] kill switch (UIの「送信を一時停止」) の動作を確認した
- [ ] UIの「ドライランを解除」を押して実送信へ切り替えた
