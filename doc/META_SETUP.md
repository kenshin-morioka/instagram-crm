# Meta App Dashboard セットアップ手順 (META_SETUP)

このアプリは「Instagram API with Instagram Login」を使用する。**Facebookページは不要**だが、開発者登録のために **Facebookアカウント (個人) は必要**。

## 1. 前提: 必要なアカウント

| アカウント | 用途 | 備考 |
|---|---|---|
| Instagramプロアカウント | APIの操作対象。トークン発行時にログインする | ビジネス or クリエイター。個人アカウントのままではAPI不可 |
| Facebookアカウント | Meta開発者ダッシュボードへのログイン | Facebook**ページ**は不要 |
| Meta for Developers登録 | 上記Facebookアカウントへの開発者権限付与 (一度きり) | 電話番号 or メールの確認あり |

- プロアカウントへの切り替え: Instagramアプリ → 設定 → アカウントの種類とツール → プロアカウントに切り替える

### 開発者登録のSMS認証が届かないとき

- 短時間に再送を繰り返すとレート制限がかかり、エラー表示なしにSMSが送られなくなる。**数時間〜24時間空けて再試行**する
- 先に facebook.com の 設定 → アカウントセンター → 個人の情報 → 連絡先情報 で電話番号を認証しておくと通りやすい
- 再試行時はシークレットウィンドウで最初からやり直す。電話番号は国コード +81 選択時、先頭の 0 を抜く

## 2. Metaアプリの作成

1. https://developers.facebook.com/apps/creation/ を開く (My Apps → Create App でも可)
2. ユースケースで **Instagram** 系 (「Instagramでビジネスを管理」等) を選択。ユースケースにInstagramが出ない場合はアプリタイプ **Business** を選択
3. ビジネスポートフォリオは「後で接続」でよい
4. 作成後、左メニュー「Instagram > API setup with Instagram business login」を開く

## 3. 認証方式

- 本アプリは `instagram_login` 方式のみ対応 (config.jsonの `meta_auth_mode`)
- Facebook Login方式 (Facebookページ紐付け・Page Access Token) は未実装

## 4. 必要な権限 (スコープ)

トークン発行時に以下を付与する。それ以外 (公開投稿・メッセージ権限等) は要求しない。

- `public_profile` (ダッシュボード上で必須として要求される)
- `instagram_business_basic` (必須。トークンリフレッシュにも必要)
- `instagram_business_manage_comments` (コメント取得・返信)

## 5. アクセストークンの発行

1. 「API setup with Instagram business login」の「1. Generate access tokens」で
   **Add account** から対象のプロアカウントを追加 (Instagramへのログインを求められる)
2. 追加したアカウント横の **Generate token** をクリックし、上記2スコープで発行
3. 表示された**長期トークン (60日有効)** をアプリの「接続状態」欄へ貼り付け

※ 旧UIでは「Token Generator」という名称で、App Roles → Instagram Testers への追加と
Instagram側 (設定 → アプリとウェブサイト → テスター招待) での承認が必要だった。
現UIで承認を求められた場合も同じ場所で承認する。

## 6. トークンの更新・失効

- 更新: アプリが `GET /refresh_access_token` で自動リフレッシュする (期限7日前から試行)。
  60日以上アプリを起動しないと失効するため、その場合は再発行して貼り直す
- 失効させたい場合: Instagramアプリの 設定 → アプリとウェブサイト から連携を解除するか、
  Meta App Dashboard でアプリを無効化する (OPERATIONS_RUNBOOK.md 参照)

## 7. アプリの「公開」への切り替え (自分用でも必須)

**開発モードのままではコメントの読み取りが常に空 (`data: []`) になる** (2026-07-27 実機確認)。
コメント投稿・削除 (POST/DELETE) や `comments_count` は成功するため権限不足と誤認しやすいが、
トークン・スコープ・テスター登録は無関係で、アプリを「公開」に切り替えるまで解消しない。

公開に必要な設定:

1. **設定 → ベーシック** で以下を入力 (未入力だと「Currently ineligible for submission」と表示される)
   - プライバシーポリシーのURL: https://kenshin-morioka.github.io/instagram-crm/privacy.html
   - 利用規約のURL: https://kenshin-morioka.github.io/instagram-crm/terms.html
   - カテゴリ: 近いものを選択 (「ビジネスと管理ページ」等)
2. **ユースケースのテスト**: 対象アプリのトークンで各権限のAPIを実際に呼び出すと実績が記録される
   (`GET /me`、`GET /me/media`、`GET·POST /{media}/comments`、`POST /{comment}/replies` 等)。
   ダッシュボードへの反映は**最大24時間**かかる。反映まで「テスト開始待ち」と表示される
3. 上記が揃うと「公開」へ切り替えられる

### App Review (アドバンスアクセス) について

- **自分のアカウントのみで使う場合、App Review (Metaの人力審査) は不要**。
  ダッシュボードにも「自分のInstagramビジネスのためにのみ構築する場合はスキップ可」と明記されている
- 不特定の第三者のアカウントで使わせる場合のみ、App Review で
  `instagram_business_manage_comments` の **Advanced Access** を申請する
- 確認箇所: App Dashboard → App Review → Permissions and Features

## 8. Webhookについて

本アプリはポーリング方式でWebhookを使用しない (サーバーレス設計のため)。
Webhook callback URL・verify token・commentsフィールド購読の設定は不要。

## 9. 本番運用前チェックリスト

- [ ] 対象アカウントがプロアカウントである
- [ ] トークンのスコープが §4 の3つのみである
- [ ] アプリを「公開」に切り替えた (開発モードのままだとコメントが取得できない)
- [ ] ドライラン (既定ON) のまま数周期動かし、`[DRY RUN]` ログで対象判定が意図どおりか確認した
- [ ] `allowed_media_ids` / `reply_keywords` を必要に応じて設定した
- [ ] 返信文が固定テンプレートとして適切 (スパム的でない) ことを確認した
- [ ] kill switch (UIの「送信を一時停止」) の動作を確認した
- [ ] UIの「ドライランを解除」を押して実送信へ切り替えた
