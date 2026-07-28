[English](README.md) | [日本語](README.ja.md)

# Instagram CRM

A desktop app that automatically replies to comments on your own Instagram Reels with a canned message, using the official API. It runs in the background on your computer, checks for new comments at a fixed interval, and replies to the ones that match your conditions.

- **Official API only** — it talks to Meta's official Instagram API (`graph.instagram.com`) and nothing else. No scraping, no unofficial APIs, no browser automation
- **Safe by design** — access tokens are stored in the OS credential store (macOS: Keychain / Windows: Credential Manager)
- **Verify before you send** — dry-run mode is the default (matches are logged, no replies are sent). You have to press a button to start sending for real
- **Supported OS: macOS / Windows** — Linux is not supported (token storage relies on the native mechanisms of these two platforms)

> This is a tool for individuals who want to streamline running their own account. Please comply with the Instagram / Meta terms of service when using it.

> **Note:** the linked documents under `docs/` are currently available in Japanese only.

---

## 1. Installation

### Prerequisites

- **An Instagram professional account** (Business or Creator). The API is not available for personal accounts
- **Setup on Meta's side** (registering an app and issuing an access token). The steps are documented in [docs/META_SETUP.md](docs/META_SETUP.md). It takes a bit of effort, but you only need to do it once

### macOS

1. Open the [Releases page](https://github.com/kenshin-morioka/instagram-crm/releases) and download the latest `.dmg` file
2. Open the downloaded `.dmg` and drag `Instagram CRM` into your **Applications** folder
3. The first time you launch the app, macOS may say it cannot be opened because the developer cannot be verified. In that case, **right-click the app icon → "Open"**, then press "Open" again in the confirmation dialog (first launch only)

### Windows

1. Open the [Releases page](https://github.com/kenshin-morioka/instagram-crm/releases) and download the latest `.msi` (installer)
2. Double-click the downloaded file and follow the on-screen instructions
3. If "Windows protected your PC" appears at launch, click **"More info" → "Run anyway"** (first launch only)

> ⚠️ These warnings appear because the app is not signed with a paid code-signing certificate. If that makes you uncomfortable, you can build it yourself by following [CONTRIBUTING.md](CONTRIBUTING.md).

---

## 2. Usage

### ① Get a Meta access token

Follow [docs/META_SETUP.md](docs/META_SETUP.md) to issue a **long-lived access token (valid for 60 days)**. Only these two permissions are needed:

- `instagram_business_basic`
- `instagram_business_manage_comments`

### ② Connect the app

1. Launch the app
2. Paste the access token from step ① into the "Connection status" field
3. Press the "Connect" button. Once the connection succeeds, the status is displayed

### ③ Check the behavior in dry-run mode (important)

The app starts in **dry-run mode (no replies are sent; matching comments are only written to the log)**. Leave it running for a few minutes to a few tens of minutes first, and use the `[DRY RUN]` lines in the log to confirm that the comments it would reply to are the ones you expect.

Log location:

- macOS: `~/Library/Logs/com.kenshinmorioka.instagram-crm/instagram-crm.log`
- Windows: `%LOCALAPPDATA%\com.kenshinmorioka.instagram-crm\logs\`

### ④ Start replying for real

Once you are satisfied, press the **"Disable dry run"** button in the app. From then on, replies are actually sent to comments that match your conditions.

### Narrowing down which comments get a reply (optional)

If replying to everything is not what you want, the settings below let you narrow the scope. The config file is created automatically on first launch.

Location (macOS): `~/Library/Application Support/com.kenshinmorioka.instagram-crm/config.json`

| Setting | Default | Description |
|---|---|---|
| `allowed_media_ids` | (empty) | Limit which Reels are eligible for replies (empty = all Reels) |
| `reply_keywords` | (empty) | Reply only when the comment body contains one of these words (empty = all comments) |
| `comment_lookback_hours` | 24 | Comments older than this are skipped (initial value; changeable from the app UI) |
| `max_comment_length` | 500 | Comments longer than this are skipped |
| `usage_pause_threshold_pct` | 90 | Pause sending once API usage exceeds this percentage |

The reply text, the check (polling) interval, and the fetch scope (number of target Reels and the comment time window) can be changed from the app UI. See [config.example.json](config.example.json) for the full list of settings.

### Troubleshooting

For an expired token, stopping replies (kill switch), and similar situations, see [docs/OPERATIONS_RUNBOOK.md](docs/OPERATIONS_RUNBOOK.md).

---

## 3. Uninstalling

Deleting the app itself leaves data such as settings and logs on your computer. To remove everything, follow the steps below.

### First: revoke the connection on Instagram (macOS / Windows, recommended)

Revoking the connection first invalidates the access token itself, which is the safer order.

- Instagram app: Settings → Apps and websites → remove the app's access

### macOS

1. Quit the app (menu bar icon → Quit)
2. Move `Instagram CRM` from the **Applications** folder to the Trash
3. Delete the leftover data. Run the following in the "Terminal" app:

   ```sh
   # settings, reply history, logs, cache
   rm -rf ~/Library/"Application Support"/com.kenshinmorioka.instagram-crm
   rm -rf ~/Library/Logs/com.kenshinmorioka.instagram-crm
   rm -rf ~/Library/Caches/com.kenshinmorioka.instagram-crm
   rm -rf ~/Library/WebKit/com.kenshinmorioka.instagram-crm

   # the access token stored in Keychain
   security delete-generic-password -s com.kenshinmorioka.instagram-crm -a instagram-token
   ```

### Windows

1. Quit the app (system tray icon → Quit)
2. Settings → Apps → Installed apps → `Instagram CRM` → Uninstall
3. Delete the leftover data. Paste the following into the Explorer address bar, open each location, and delete the `com.kenshinmorioka.instagram-crm` folder:
   - `%APPDATA%` (settings, reply history)
   - `%LOCALAPPDATA%` (logs, WebView data)
4. Delete the stored access token:
   - Control Panel → Credential Manager → Windows Credentials →
     delete the entries containing `com.kenshinmorioka.instagram-crm`

---

## 4. Quality and safety

The app is built along the following principles so that you can use it with confidence.

- **Official API only** — the only outbound communication is to Meta's official API (`graph.instagram.com`). No unofficial APIs, no scraping, no browser automation
- **Minimal permissions** — only the two permissions required to read and reply to comments are requested. No permissions for publishing posts or sending messages
- **Tokens stored securely** — access tokens are kept in the OS credential store. They are never written to config files or logs
- **Dry run by default** — to prevent accidental sends, no real replies happen until you explicitly press the button
- **No duplicate replies** — replied comments are recorded, so the same comment never gets a second reply
- **Privacy-conscious logging** — token values, comment bodies, and usernames are never written to the log
- **Respectful of API limits** — it backs off automatically on rate limits (HTTP 429), and pauses automatically once usage exceeds the threshold

The results of a static review from a terms-compliance perspective, available for third parties to inspect, are documented in [docs/COMPLIANCE_AUDIT.md](docs/COMPLIANCE_AUDIT.md).

---

## 5. Issues and feedback welcome

This app is published as open source. **If anything catches your attention, please feel free to open an [Issue](https://github.com/kenshin-morioka/instagram-crm/issues) about it — anything at all.**

- Basic things like "I got stuck during installation" or "this explanation is hard to follow" are **very welcome**
- Bug reports, feature requests, and improvement suggestions are all welcome
- Questions like "I'd like to use it this way — is that possible?" are welcome too

Pull requests are welcome as well 🙌 If you want to contribute, see [CONTRIBUTING.md](CONTRIBUTING.md).

---

## 6. License

Released under the [MIT License](LICENSE). You are free to modify and redistribute it, but **this software is provided without warranty, and the author is not liable for any damages arising from its use.** Use it at your own risk.
