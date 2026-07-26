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

トークン発行時に以下のみ付与する。それ以外 (公開投稿・メッセージ権限等) は要求しない。

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
