const { invoke } = window.__TAURI__.core;
const { getVersion } = window.__TAURI__.app;
const { check: checkUpdate } = window.__TAURI__.updater;
const { relaunch } = window.__TAURI__.process;

const statusDot = document.getElementById("status-dot");
const statusText = document.getElementById("status-text");
const modeBadge = document.getElementById("mode-badge");
const modeHint = document.getElementById("mode-hint");
const dryRunButton = document.getElementById("dry-run-button");
const pausedBadge = document.getElementById("paused-badge");
const pauseButton = document.getElementById("pause-button");
const tokenForm = document.getElementById("token-form");
const tokenInput = document.getElementById("token-input");
const connectButton = document.getElementById("connect-button");
const replyText = document.getElementById("reply-text");
const saveReply = document.getElementById("save-reply");
const intervalHours = document.getElementById("interval-hours");
const intervalMinutes = document.getElementById("interval-minutes");
const intervalSeconds = document.getElementById("interval-seconds");
const saveInterval = document.getElementById("save-interval");
const lastRun = document.getElementById("last-run");
const pollingHelp = document.getElementById("polling-help");
const pollingHint = document.getElementById("polling-hint");
const issueBadge = document.getElementById("issue-badge");
const issueHint = document.getElementById("issue-hint");
const progressLine = document.getElementById("progress-line");
const progressText = document.getElementById("progress-text");
const truncatedNote = document.getElementById("truncated-note");
const cycleSummary = document.getElementById("cycle-summary");
const fetchLimit = document.getElementById("fetch-limit");
const lookbackHours = document.getElementById("lookback-hours");
const saveFetch = document.getElementById("save-fetch");
const fetchHelp = document.getElementById("fetch-help");
const fetchHint = document.getElementById("fetch-hint");
const message = document.getElementById("message");
const appVersion = document.getElementById("app-version");
const updateBanner = document.getElementById("update-banner");
const updateText = document.getElementById("update-text");
const updateButton = document.getElementById("update-button");
const mainContent = document.querySelector("main");
const termsOverlay = document.getElementById("terms-overlay");
const termsCheckbox = document.getElementById("terms-checkbox");
const termsAccept = document.getElementById("terms-accept");

const STATUS_VIEW = {
  connected: { text: "接続済み", dotClass: "connected", showConnect: false },
  not_connected: { text: "未接続", dotClass: "", showConnect: true },
  needs_reauth: {
    text: "トークンの再設定が必要です",
    dotClass: "needs-reauth",
    showConnect: true,
  },
};

function showMessage(text) {
  message.textContent = text;
  message.hidden = false;
  setTimeout(() => {
    message.hidden = true;
  }, 4000);
}

let sendingPaused = false;
let dryRun = true;
let connecting = false;

function renderStatus(payload) {
  const view = STATUS_VIEW[payload.status] ?? STATUS_VIEW.not_connected;
  statusText.textContent = view.text;
  statusDot.className = `dot ${view.dotClass}`;
  tokenForm.hidden = !view.showConnect;
  lastRun.textContent = payload.last_run_at
    ? `最終チェック: ${payload.last_run_at}`
    : "-";

  // 実行中はスピナー付きで進捗を出す
  progressLine.hidden = !payload.progress;
  progressText.textContent = payload.progress ?? "";

  // 直近の周期で何を確認したか (「本当に動いてる？」への答え)
  cycleSummary.hidden = !payload.last_cycle_summary;
  cycleSummary.textContent = payload.last_cycle_summary ?? "";

  // 一時エラー連続時の警告と、コメント確認しきれなかった可能性の通知
  issueBadge.hidden = !payload.connection_issue;
  issueHint.hidden = !payload.connection_issue;
  truncatedNote.hidden = !payload.fetch_truncated;

  sendingPaused = payload.sending_paused;
  dryRun = payload.dry_run;
  pausedBadge.hidden = !payload.sending_paused;

  const connected = payload.status === "connected";
  pauseButton.hidden = !connected;
  pauseButton.textContent = payload.sending_paused ? "送信を再開" : "送信を一時停止";

  modeBadge.textContent = payload.dry_run ? "ドライラン中 (送信なし)" : "実送信中";
  modeBadge.className = `mode-badge ${payload.dry_run ? "mode-dry" : "mode-live"}`;
  modeHint.hidden = !payload.dry_run;
  dryRunButton.textContent = payload.dry_run ? "実送信を開始する" : "ドライランに戻す";
  // 実送信の開始は目立たせ、テストへ戻す操作は控えめにする
  dryRunButton.className = payload.dry_run ? "btn btn-gradient" : "btn";
  dryRunButton.disabled = !connected;
}

async function refreshStatus() {
  // トークン検証中の表示を周期更新で「未接続」へ巻き戻さない
  if (connecting) return;
  try {
    renderStatus(await invoke("get_status"));
  } catch (e) {
    console.error(e);
  }
}

// バックエンドの上下限 (state.rs の定数) と同じ値
const MIN_INTERVAL_SECS = 30;
const MAX_INTERVAL_SECS = 43200;

function displayInterval(secs) {
  intervalHours.value = Math.floor(secs / 3600);
  intervalMinutes.value = Math.floor((secs % 3600) / 60);
  intervalSeconds.value = secs % 60;
}

async function loadSettings() {
  const settings = await invoke("get_settings");
  replyText.value = settings.reply_text;
  displayInterval(settings.polling_interval_secs);
  fetchLimit.value = settings.media_fetch_limit;
  lookbackHours.value = settings.comment_lookback_hours;
}

connectButton.addEventListener("click", async () => {
  connecting = true;
  connectButton.disabled = true;
  statusText.textContent = "トークンを検証しています...";
  try {
    renderStatus(await invoke("connect_with_token", { token: tokenInput.value }));
    showMessage("Instagramと連携しました");
  } catch (e) {
    showMessage(String(e));
    connecting = false;
    await refreshStatus();
  } finally {
    // 失敗時も含めトークンを入力欄に残さない
    tokenInput.value = "";
    connecting = false;
    connectButton.disabled = false;
  }
});

dryRunButton.addEventListener("click", async () => {
  if (
    dryRun &&
    !confirm("実送信を開始すると、対象コメントへの実際の自動返信が始まります。よろしいですか？")
  ) {
    return;
  }
  // renderStatusがdryRun/sendingPausedを切替後の値で上書きするため、
  // トースト文言の判定には切替前の値を使う
  const wasDryRun = dryRun;
  try {
    renderStatus(await invoke("set_dry_run", { enabled: !dryRun }));
    showMessage(wasDryRun ? "実送信を開始しました" : "ドライランに戻しました (送信なし)");
  } catch (e) {
    showMessage(String(e));
  }
});

pauseButton.addEventListener("click", async () => {
  if (
    sendingPaused &&
    !confirm("送信を再開すると、対象コメントへの自動返信が再び始まります。よろしいですか？")
  ) {
    return;
  }
  const wasPaused = sendingPaused;
  try {
    renderStatus(await invoke("set_sending_paused", { paused: !sendingPaused }));
    showMessage(wasPaused ? "送信を再開しました" : "送信を一時停止しました");
  } catch (e) {
    showMessage(String(e));
  }
});

saveReply.addEventListener("click", async () => {
  try {
    await invoke("save_reply_text", { text: replyText.value });
    showMessage("返信文を保存しました");
  } catch (e) {
    showMessage(String(e));
  }
});

pollingHelp.addEventListener("click", () => {
  pollingHint.hidden = !pollingHint.hidden;
});

fetchHelp.addEventListener("click", () => {
  fetchHint.hidden = !fetchHint.hidden;
});

// バックエンドの上下限 (state.rs の定数) と同じ値
const MIN_FETCH_LIMIT = 1;
const MAX_FETCH_LIMIT = 25;
const MIN_LOOKBACK_HOURS = 1;
const MAX_LOOKBACK_HOURS = 168;

saveFetch.addEventListener("click", async () => {
  const limit = Number(fetchLimit.value);
  const hours = Number(lookbackHours.value);
  // 小数や数値以外はバックエンドのserdeエラー (英語) がそのまま出るため先に弾く
  if (!Number.isInteger(limit) || limit < MIN_FETCH_LIMIT || limit > MAX_FETCH_LIMIT) {
    showMessage(`対象リール数は${MIN_FETCH_LIMIT}〜${MAX_FETCH_LIMIT}件で入力してください`);
    return;
  }
  if (!Number.isInteger(hours) || hours < MIN_LOOKBACK_HOURS || hours > MAX_LOOKBACK_HOURS) {
    showMessage(
      `コメント対象期間は${MIN_LOOKBACK_HOURS}〜${MAX_LOOKBACK_HOURS}時間で入力してください`
    );
    return;
  }
  try {
    await invoke("save_fetch_settings", {
      mediaFetchLimit: limit,
      commentLookbackHours: hours,
    });
    showMessage("取得範囲を保存しました");
  } catch (e) {
    showMessage(String(e));
  }
});

saveInterval.addEventListener("click", async () => {
  // 空欄は0として扱う
  const hours = Number(intervalHours.value || 0);
  const minutes = Number(intervalMinutes.value || 0);
  const seconds = Number(intervalSeconds.value || 0);
  // 小数や数値以外はバックエンドのserdeエラー (英語) がそのまま出るため先に弾く
  const parts = [hours, minutes, seconds];
  if (parts.some((v) => !Number.isInteger(v) || v < 0)) {
    showMessage("ポーリング間隔は0以上の整数で入力してください");
    return;
  }
  const secs = hours * 3600 + minutes * 60 + seconds;
  if (secs < MIN_INTERVAL_SECS) {
    showMessage("ポーリング間隔は合計30秒以上にしてください");
    return;
  }
  if (secs > MAX_INTERVAL_SECS) {
    showMessage("ポーリング間隔は合計12時間以内にしてください");
    return;
  }
  try {
    await invoke("save_polling_interval", { secs });
    // 「90分」のような入力も保存後は正規化して表示し直す (→1時間30分)
    displayInterval(secs);
    showMessage("ポーリング間隔を保存しました");
  } catch (e) {
    showMessage(String(e));
  }
});

// ---------- アップデート ----------

let pendingUpdate = null;

async function checkForUpdate() {
  // ダウンロード中・案内表示中は再チェックしない
  if (pendingUpdate) return;
  try {
    const update = await checkUpdate();
    if (!update) return;
    pendingUpdate = update;
    updateText.textContent = `新しいバージョン v${update.version} が利用できます`;
    updateBanner.hidden = false;
  } catch (e) {
    // オフライン時やGitHub障害時は次回のチェックに任せて黙って続行する
    console.error("アップデート確認に失敗:", e);
  }
}

updateButton.addEventListener("click", async () => {
  if (!pendingUpdate) return;
  updateButton.disabled = true;
  updateButton.textContent = "ダウンロード中...";
  try {
    await pendingUpdate.downloadAndInstall();
    await relaunch();
  } catch (e) {
    showMessage(`アップデートに失敗しました: ${e}`);
    updateButton.disabled = false;
    updateButton.textContent = "最新バージョンをインストール";
  }
});

getVersion()
  .then((v) => {
    appVersion.textContent = `Instagram CRM v${v}`;
  })
  .catch((e) => console.error(e));

// オーバーレイ表示中は背面UIをinertにしてキーボードフォーカスも隔離する
function setTermsOverlayVisible(visible) {
  termsOverlay.hidden = !visible;
  mainContent.inert = visible;
  if (visible) termsCheckbox.focus();
}

termsCheckbox.addEventListener("change", () => {
  termsAccept.disabled = !termsCheckbox.checked;
});

termsAccept.addEventListener("click", async () => {
  try {
    await invoke("accept_terms");
    setTermsOverlayVisible(false);
  } catch (e) {
    showMessage(String(e));
  }
});

// 未同意なら同意ダイアログでUI全体を覆う。
// 判定が完了するまで・判定に失敗した場合も未同意扱い (安全側)
async function showTermsIfNeeded() {
  setTermsOverlayVisible(true);
  try {
    const accepted = await invoke("get_terms_accepted");
    if (accepted) setTermsOverlayVisible(false);
  } catch (e) {
    console.error(e);
  }
}

showTermsIfNeeded();
loadSettings().catch((e) => showMessage(String(e)));
refreshStatus();
// 2秒間隔なのはポーリング中の進捗表示を取りこぼさないため (ローカルIPCのみで軽量)
setInterval(refreshStatus, 2000);
checkForUpdate();
// 常駐アプリのため起動時だけでなく6時間ごとにも確認する
setInterval(checkForUpdate, 6 * 60 * 60 * 1000);
