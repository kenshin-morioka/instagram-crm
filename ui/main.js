const { invoke } = window.__TAURI__.core;

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
const pollingInterval = document.getElementById("polling-interval");
const pollingUnit = document.getElementById("polling-unit");
const saveInterval = document.getElementById("save-interval");
const lastRun = document.getElementById("last-run");
const pollingHelp = document.getElementById("polling-help");
const pollingHint = document.getElementById("polling-hint");
const message = document.getElementById("message");

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
  lastRun.textContent = payload.last_run_at ?? "-";

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

// バックエンドの下限30秒・上限12時間 (state.rs の定数) に合わせた単位別の入力制限
const MAX_INTERVAL_SECS = 43200;
const UNIT_LIMITS = {
  1: { min: 30, step: 10 },
  60: { min: 1, step: 1 },
  3600: { min: 1, step: 1 },
};

function applyUnitLimits() {
  const unit = Number(pollingUnit.value);
  const limits = UNIT_LIMITS[unit];
  pollingInterval.min = limits.min;
  pollingInterval.step = limits.step;
  pollingInterval.max = MAX_INTERVAL_SECS / unit;
}

// 秒数を割り切れる最大の単位 (時間 > 分 > 秒) で表示する
function displayInterval(secs) {
  const unit = secs % 3600 === 0 ? 3600 : secs % 60 === 0 ? 60 : 1;
  pollingUnit.value = String(unit);
  pollingInterval.value = secs / unit;
  applyUnitLimits();
}

async function loadSettings() {
  const settings = await invoke("get_settings");
  replyText.value = settings.reply_text;
  displayInterval(settings.polling_interval_secs);
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

pollingUnit.addEventListener("change", applyUnitLimits);

saveInterval.addEventListener("click", async () => {
  const value = Number(pollingInterval.value);
  // 小数や数値以外はバックエンドのserdeエラー (英語) がそのまま出るため先に弾く
  if (!Number.isInteger(value)) {
    showMessage("ポーリング間隔は整数で入力してください");
    return;
  }
  const secs = value * Number(pollingUnit.value);
  try {
    await invoke("save_polling_interval", { secs });
    showMessage("ポーリング間隔を保存しました");
  } catch (e) {
    showMessage(String(e));
  }
});

loadSettings().catch((e) => showMessage(String(e)));
refreshStatus();
setInterval(refreshStatus, 5000);
