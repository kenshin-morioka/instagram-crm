const { invoke } = window.__TAURI__.core;

const statusDot = document.getElementById("status-dot");
const statusText = document.getElementById("status-text");
const dryRunBadge = document.getElementById("dry-run-badge");
const dryRunButton = document.getElementById("dry-run-button");
const pausedBadge = document.getElementById("paused-badge");
const pauseButton = document.getElementById("pause-button");
const tokenForm = document.getElementById("token-form");
const tokenInput = document.getElementById("token-input");
const connectButton = document.getElementById("connect-button");
const replyText = document.getElementById("reply-text");
const saveReply = document.getElementById("save-reply");
const pollingInterval = document.getElementById("polling-interval");
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

function renderStatus(payload) {
  const view = STATUS_VIEW[payload.status] ?? STATUS_VIEW.not_connected;
  statusText.textContent = view.text;
  statusDot.className = `dot ${view.dotClass}`;
  tokenForm.hidden = !view.showConnect;
  lastRun.textContent = payload.last_run_at ?? "-";

  sendingPaused = payload.sending_paused;
  dryRun = payload.dry_run;
  dryRunBadge.hidden = !payload.dry_run;
  pausedBadge.hidden = !payload.sending_paused;

  const connected = payload.status === "connected";
  pauseButton.hidden = !connected;
  pauseButton.textContent = payload.sending_paused ? "送信を再開" : "送信を一時停止";
  dryRunButton.hidden = !connected;
  dryRunButton.textContent = payload.dry_run ? "ドライランを解除" : "ドライランに戻す";
}

async function refreshStatus() {
  try {
    renderStatus(await invoke("get_status"));
  } catch (e) {
    console.error(e);
  }
}

async function loadSettings() {
  const settings = await invoke("get_settings");
  replyText.value = settings.reply_text;
  pollingInterval.value = settings.polling_interval_secs;
}

connectButton.addEventListener("click", async () => {
  connectButton.disabled = true;
  statusText.textContent = "トークンを検証しています...";
  try {
    renderStatus(await invoke("connect_with_token", { token: tokenInput.value }));
    tokenInput.value = "";
    showMessage("Instagramと連携しました");
  } catch (e) {
    showMessage(String(e));
    await refreshStatus();
  } finally {
    connectButton.disabled = false;
  }
});

dryRunButton.addEventListener("click", async () => {
  if (
    dryRun &&
    !confirm("ドライランを解除すると、対象コメントへの実際の自動返信が始まります。よろしいですか？")
  ) {
    return;
  }
  // renderStatusがdryRun/sendingPausedを切替後の値で上書きするため、
  // トースト文言の判定には切替前の値を使う
  const wasDryRun = dryRun;
  try {
    renderStatus(await invoke("set_dry_run", { enabled: !dryRun }));
    showMessage(wasDryRun ? "ドライランを解除しました (実送信が始まります)" : "ドライランに戻しました");
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

saveInterval.addEventListener("click", async () => {
  try {
    await invoke("save_polling_interval", { secs: Number(pollingInterval.value) });
    showMessage("ポーリング間隔を保存しました");
  } catch (e) {
    showMessage(String(e));
  }
});

loadSettings().catch((e) => showMessage(String(e)));
refreshStatus();
setInterval(refreshStatus, 5000);
