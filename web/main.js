// The shell's own page: the launch dialog, progress, and failures.
//
// Two steps, because the port is the user's call. Rust discovers what is already
// running and works out a port to suggest (`shell://choose`); this page shows it
// and waits. Nothing is started until the user confirms, and only once a session
// exists (`shell://ready`) does this page ask for the dsh window through
// `open_dsh` — which builds a **separate** window. This page never navigates
// anywhere, which is the point: a shell page navigating to dsh would be a
// cross-site navigation, and dsh's `SameSite=Strict` cookie would be withheld.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = (id) => document.getElementById(id);

/** Latest state snapshot pushed by the shell. */
let shell = null;
/** Latest dsh update payload. */
let update = null;
/** Latest self-update payload — about this application, not about dsh. */
let selfUpdate = null;
/** Whether an update check has finished (successfully or not). */
let updateSettled = false;
/** Version currently highlighted in the dialog. */
let selected = null;
/** The version the "don't remind me" checkbox applies to. */
let dismissTarget = null;
/** The webview has been handed to the dsh UI. */
let navigated = false;
/** The launch dialog is on screen. */
let dialogOpen = false;
/** The port was confirmed and a server is being started for it. */
let starting = false;

// ── failure reporting ──────────────────────────────────────────────────────

// A packaged GUI app has no terminal, so a page that dies silently would look
// like the shell simply stopped. These go to the harness's stderr through a
// command, which is ordinary application code on the shell's own page — nothing
// is ever injected into the dsh origin.
let reports = 0;

function reportFailure(stage, message) {
  if (reports > 20) return;
  reports += 1;
  try {
    const pending = invoke("page_diag", { stage, message: String(message) });
    if (pending && typeof pending.catch === "function") pending.catch(() => {});
  } catch (error) {
    /* no bridge at all */
  }
}

window.addEventListener(
  "error",
  (event) => {
    const target = event.target;
    if (target && target !== window && (target.src || target.href)) {
      reportFailure("resource-error", `${target.tagName || "?"} ${target.src || target.href}`);
    } else {
      reportFailure("error", `${event.message || "?"} @ ${event.filename || "?"}:${event.lineno || 0}`);
    }
  },
  true,
);

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  reportFailure("unhandledrejection", reason && reason.message ? reason.message : reason);
});

// ── version comparison (mirrors the Rust side; used only for row badges) ────

function parseVersion(value) {
  const match = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(String(value || ""));
  if (!match) return null;
  return { nums: [+match[1], +match[2], +match[3]], pre: match[4] ? match[4].split(".") : [] };
}

function compareVersions(a, b) {
  const left = parseVersion(a);
  const right = parseVersion(b);
  if (!left || !right) return a === b ? 0 : a < b ? -1 : 1;
  for (let i = 0; i < 3; i += 1) {
    if (left.nums[i] !== right.nums[i]) return left.nums[i] - right.nums[i];
  }
  if (!left.pre.length && !right.pre.length) return 0;
  if (!left.pre.length) return 1;
  if (!right.pre.length) return -1;
  const len = Math.max(left.pre.length, right.pre.length);
  for (let i = 0; i < len; i += 1) {
    const p = left.pre[i];
    const q = right.pre[i];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const pNum = /^\d+$/.test(p);
    const qNum = /^\d+$/.test(q);
    if (pNum && qNum) {
      if (+p !== +q) return +p - +q;
    } else if (pNum !== qNum) {
      return pNum ? -1 : 1;
    } else if (p !== q) {
      return p < q ? -1 : 1;
    }
  }
  return 0;
}

function formatDate(iso) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (n) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

// ── views ──────────────────────────────────────────────────────────────────

function renderSplash() {
  if (!shell) return;
  el("splash-message").textContent = shell.message || "正在启动…";
}

/** Show exactly one of the shell's three stages. */
function showStage(name) {
  for (const id of ["splash", "dialog", "failure"]) {
    el(id).classList.toggle("hidden", id !== name);
  }
}

function renderFailure() {
  dialogOpen = false;
  showStage("failure");
  el("failure-message").textContent = shell?.error || "未知错误";
  el("failure-logdir").textContent = shell?.log_dir || "~/.dsh/launcher/logs";
}

function showInlineError(message) {
  const box = el("check-error");
  box.textContent = message;
  box.classList.remove("hidden");
}

function versionRow(entry, current) {
  const row = document.createElement("li");
  row.className = "version";
  const isCurrent = entry.version === current;
  if (isCurrent) row.classList.add("installed");
  else if (entry.version === selected) row.classList.add("selected");

  const label = document.createElement("span");
  label.textContent = entry.version;
  row.append(label);

  const meta = document.createElement("span");
  meta.className = "meta";

  const date = document.createElement("span");
  date.className = "muted";
  date.textContent = formatDate(entry.published);
  meta.append(date);

  if (isCurrent) {
    const badge = document.createElement("span");
    badge.className = "badge current";
    badge.textContent = "已安装";
    meta.append(badge);
  } else if (compareVersions(entry.version, current) > 0) {
    const badge = document.createElement("span");
    badge.className = "badge new";
    badge.textContent = "可更新";
    meta.append(badge);
  }
  row.append(meta);

  if (!isCurrent) {
    row.addEventListener("click", () => {
      selected = entry.version;
      renderChannels();
      updateButtons();
    });
  }
  return row;
}

function renderChannels() {
  const host = el("channels");
  host.replaceChildren();
  const report = update?.report;
  if (!report) {
    const empty = document.createElement("p");
    empty.className = "empty";
    // The dialog opens before the registry answers, so "nothing yet" and
    // "nothing at all" have to read differently.
    empty.textContent = updateSettled ? "没有可用的版本信息。" : "正在检查更新…";
    host.append(empty);
    return;
  }

  for (const channel of report.channels) {
    const section = document.createElement("section");
    section.className = "channel";
    section.dataset.channel = channel.id;

    const head = document.createElement("div");
    head.className = "channel-head";

    const name = document.createElement("span");
    name.className = "channel-name";
    name.textContent = channel.label;
    head.append(name);

    if (channel.dist_tags.length) {
      const tags = document.createElement("span");
      tags.className = "tags";
      for (const tag of channel.dist_tags) {
        const chip = document.createElement("span");
        chip.className = "tag";
        chip.textContent = tag;
        tags.append(chip);
      }
      head.append(tags);
    }
    section.append(head);

    const list = document.createElement("ul");
    list.className = "versions";
    if (!channel.versions.length) {
      const empty = document.createElement("li");
      empty.className = "empty";
      empty.textContent = "该通道暂无版本";
      list.append(empty);
    }
    for (const entry of channel.versions) list.append(versionRow(entry, report.current));
    section.append(list);
    host.append(section);
  }
}

function updateButtons() {
  const current = update?.report?.current;
  const button = el("btn-update");
  const runnable = Boolean(selected) && selected !== current;
  button.disabled = !runnable;
  button.textContent = runnable ? `更新到 ${selected} 并重启` : "更新并重启";
}

/**
 * Render the shell's own update line.
 *
 * Called whenever the payload arrives or the user dismisses a version; it also
 * fills the header's version, which is the only place this build's own version
 * is named.
 */
function renderSelfUpdate() {
  const current = selfUpdate?.current || shell?.shell_version || "";
  el("shell-version").textContent = current || "—";

  const box = el("self-update");
  const version = selfUpdate?.version;
  if (!version || !selfUpdate?.should_prompt) {
    box.classList.add("hidden");
    return;
  }

  el("self-update-text").textContent = `外壳有新版本 ${version}（当前 ${current}）`;
  const button = el("btn-self-update");
  button.textContent = selfUpdate.can_install ? "下载并安装" : "打开发布页";
  button.disabled = false;
  button.classList.remove("hidden");
  el("btn-self-dismiss").classList.remove("hidden");
  box.classList.remove("hidden");
}

/** Open the launch dialog. Re-opening it does not disturb what is typed. */
function showDialog() {
  if (dialogOpen) return;
  dialogOpen = true;

  // The default is a placeholder rather than a value: confirming without typing
  // has to be the fast path, and the grey digits say which port that will be.
  const input = el("port-input");
  input.value = "";
  input.placeholder = shell?.default_port ? String(shell.default_port) : "";
  setPortError("");
  setPortHint();

  renderDialog();
  renderSelfUpdate();
  showStage("dialog");
  input.focus();
}

/** Re-render everything in the dialog that is derived from state. */
function renderDialog() {
  const report = update?.report;
  el("current-version").textContent = report?.current || shell?.current_version || "—";
  // The resolved launcher. This line used to show the loopback URL, which is not
  // where dsh is installed.
  el("dsh-location").textContent = shell?.dsh_bin || "—";
  if (update?.error) showInlineError(`更新检查失败：${update.error}`);
  else el("check-error").classList.add("hidden");
  el("dismiss-target").textContent = dismissTarget || "—";
  renderChannels();
  updateButtons();
}

/**
 * Take a fresh update payload.
 *
 * The candidate is only adopted when the user has not already chosen one, so a
 * payload that lands while they are reading does not move the selection under
 * them. A recheck clears the choice first, so a recheck does adopt.
 */
function adoptUpdate(payload) {
  update = payload;
  updateSettled = true;
  if (!dismissTarget) {
    dismissTarget = payload?.report?.candidate?.version || null;
    selected = dismissTarget;
    el("dismiss-check").checked = false;
  }
  renderDialog();
}

// ── the port choice ────────────────────────────────────────────────────────

/** The port the field will use: what was typed, else the greyed default. */
function chosenPort() {
  const typed = el("port-input").value.trim();
  const raw = typed === "" ? String(shell?.default_port ?? "") : typed;
  const port = Number.parseInt(raw, 10);
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : null;
}

/** Say what the port in the field will do, as far as that is knowable yet. */
function setPortHint(kind, port) {
  const hint = el("port-hint");
  const target = port ?? chosenPort();
  if (kind === "reuse") {
    hint.textContent = `端口 ${target} 上已有 dsh 服务，将直接复用。`;
    return;
  }
  if (kind === "start") {
    hint.textContent = shell?.running_port
      ? `将在端口 ${target} 上再启动一个实例；端口 ${shell.running_port} 上的服务保持不动。`
      : `将在端口 ${target} 上启动 dsh 服务。`;
    return;
  }
  hint.textContent = shell?.running_port
    ? `端口 ${shell.running_port} 已有服务，留空即复用它；填别的端口会另起一个实例。`
    : shell?.default_port
      ? `留空即在端口 ${shell.default_port} 启动；灰色数字是默认值。`
      : "留空即自动选择端口。";
}

/** Show or clear the port error line. */
function setPortError(message) {
  const box = el("port-error");
  if (!message) {
    box.classList.add("hidden");
    return;
  }
  box.textContent = message;
  box.classList.remove("hidden");
}

/**
 * Confirm the port, then start.
 *
 * The port is checked first, so one that something else owns is answered inside
 * the dialog instead of after a server boot that was never going to work.
 */
async function confirmAndStart() {
  const button = el("btn-open");
  setPortError("");
  const port = chosenPort();
  if (port === null) {
    setPortError("请输入 1–65535 之间的端口。");
    return;
  }

  const label = button.textContent;
  button.disabled = true;
  button.textContent = "检查端口…";
  let verdict;
  try {
    verdict = await invoke("check_port", { port });
  } catch (failure) {
    setPortError(`无法检查端口：${failure}`);
    return;
  } finally {
    button.disabled = false;
    button.textContent = label;
  }

  if (verdict.kind === "occupied") {
    setPortError(`端口 ${port} 已被其他程序占用，换一个或留空。`);
    return;
  }
  if (verdict.kind === "foreign") {
    // Not "occupied": a dsh is there, it just is not one this machine can enter
    // — a Windows-side instance seen from WSL, or one started under another
    // DSH_HOME. Saying so avoids "that's my dsh!" being answered with "no".
    setPortError(
      `端口 ${port} 上有一个 dsh 服务，但它的会话不在本机（例如在 Windows 侧或用另一个 DSH_HOME 启动的），无法复用。` +
        "换一个端口，或先停掉它。",
    );
    return;
  }
  if (verdict.kind === "too-low") {
    setPortError(`端口 ${port} 低于 1024，普通用户无法绑定。`);
    return;
  }

  // The update question lives in this dialog now, so it is answered here too.
  if (el("dismiss-check").checked && dismissTarget) {
    try {
      await invoke("dismiss_version", { version: dismissTarget });
    } catch (failure) {
      showInlineError(String(failure));
      return;
    }
  }

  setPortHint(verdict.kind, port);
  dialogOpen = false;
  starting = true;
  // Back to the splash: it carries the progress line while the server boots.
  showStage("splash");
  el("splash-message").textContent = `正在端口 ${port} 启动 dsh 服务…`;
  try {
    // Returns as soon as the work is under way; the outcome arrives as an event.
    // `typed` says whether the port came from the field or from the greyed
    // default, which the value alone cannot: typing the suggested port by hand is
    // still a preference, and Rust remembers only preferences.
    const typed = el("port-input").value.trim() !== "";
    await invoke("start_server", { port, typed });
  } catch (failure) {
    reportFailure("start_server", failure);
    starting = false;
    shell = { ...(shell || {}), phase: "failed", error: `无法启动 dsh 服务：${failure}` };
    renderFailure();
  }
}

// ── flow ───────────────────────────────────────────────────────────────────

async function goToDsh() {
  if (navigated) return;
  navigated = true;
  // The bootstrap window stays on screen until Rust has shown the dsh window,
  // so this line is what the user reads while the guest loads.
  el("splash-message").textContent = "正在载入 dsh 界面…";
  try {
    await invoke("open_dsh");
  } catch (error) {
    // Surfacing this inside the dialog would be invisible once the dialog is
    // gone, which is exactly when this runs — so it becomes a failure page.
    // It also goes to the harness's stderr: this is the failure that decides
    // whether the app is usable at all, and a packaged GUI has no terminal.
    reportFailure("open_dsh", error);
    navigated = false;
    shell = { ...(shell || {}), phase: "failed", error: `无法载入 dsh 界面：${error}` };
    renderFailure();
  }
}

/** Fold a state snapshot into the page. */
function applySnapshot(state) {
  if (!state) return;
  shell = state;
  if (state.phase === "failed") {
    renderFailure();
    return;
  }
  renderSplash();
  if (state.self_update) {
    selfUpdate = state.self_update;
    renderSelfUpdate();
  }
  if (state.update || state.update_error) {
    adoptUpdate({
      current: state.current_version || "",
      report: state.update || null,
      error: state.update_error || null,
      should_prompt: Boolean(state.update?.candidate && !state.update?.candidate_dismissed),
    });
  }
}

/**
 * React to a snapshot: pick the stage, or hand over once a session exists.
 *
 * One place decides which of the three is on screen, so the stages can never
 * stack up on each other.
 */
function decide() {
  if (navigated || !shell) return;
  if (shell.phase === "failed") {
    showStage("failure");
    return;
  }
  if (shell.phase === "choosing") {
    // `starting` guards the window between confirming and Rust moving the phase
    // on: a status snapshot from before that must not re-open the dialog.
    if (!starting) showDialog();
    return;
  }
  if (shell.phase === "ready") {
    goToDsh();
    return;
  }
  // Starting: the splash carries the progress line. Not while the dialog is up,
  // which only happens if an older status snapshot arrives late.
  if (!dialogOpen) showStage("splash");
}

/**
 * Subscribe to the shell's events.
 *
 * `core:event:listen` is a capability, and a missing one rejects here. That
 * must not take the whole page down — losing live progress is survivable,
 * losing the hand-off is not — so the caller falls back to polling.
 */
async function attachListeners() {
  const handlers = [
    ["shell://status", (event) => { shell = event.payload; renderSplash(); decide(); }],
    ["shell://choose", (event) => { shell = event.payload; renderSplash(); decide(); }],
    ["shell://ready", (event) => { shell = event.payload; decide(); }],
    ["shell://error", (event) => { shell = event.payload; renderFailure(); }],
    ["shell://update", (event) => adoptUpdate(event.payload)],
    ["shell://self-update", (event) => { selfUpdate = event.payload; renderSelfUpdate(); }],
  ];
  try {
    for (const [name, handler] of handlers) await listen(name, handler);
    return true;
  } catch (error) {
    console.error("事件订阅失败，改为轮询:", error);
    return false;
  }
}

function startPolling() {
  setInterval(async () => {
    if (navigated) return;
    try {
      applySnapshot(await invoke("get_state"));
      decide();
    } catch (error) {
      console.error("轮询失败:", error);
    }
  }, 1000);
}

// ── wiring ─────────────────────────────────────────────────────────────────

el("btn-open").addEventListener("click", confirmAndStart);

el("btn-update").addEventListener("click", async () => {
  if (!selected) return;
  el("progress-text").textContent = `正在更新到 ${selected}…`;
  el("progress").classList.remove("hidden");
  try {
    // On success the shell restarts, so this promise does not resolve.
    await invoke("apply_update", { version: selected });
  } catch (error) {
    el("progress").classList.add("hidden");
    showInlineError(String(error));
  }
});

el("btn-recheck").addEventListener("click", async () => {
  const button = el("btn-recheck");
  button.disabled = true;
  button.textContent = "检查中…";
  try {
    // A recheck is a fresh question, so it is allowed to replace the candidate
    // and to clear the checkbox that silences it.
    dismissTarget = null;
    updateSettled = false;
    renderDialog();
    adoptUpdate(await invoke("check_updates"));
  } catch (error) {
    showInlineError(String(error));
  } finally {
    button.disabled = false;
    button.textContent = "重新检查";
  }
});

el("btn-self-update").addEventListener("click", async () => {
  const button = el("btn-self-update");
  const label = button.textContent;
  button.disabled = true;
  button.textContent = "正在下载…";
  try {
    // Verified against the release's SHA256SUMS before it is opened.
    el("self-update-text").textContent = await invoke("apply_self_update");
    button.classList.add("hidden");
    el("btn-self-dismiss").classList.add("hidden");
  } catch (failure) {
    reportFailure("apply_self_update", failure);
    el("self-update-text").textContent = `外壳更新失败：${failure}`;
    button.disabled = false;
    button.textContent = label;
  }
});

el("btn-self-dismiss").addEventListener("click", async () => {
  const version = selfUpdate?.version;
  if (!version) return;
  try {
    await invoke("dismiss_self_version", { version });
    selfUpdate = { ...(selfUpdate || {}), should_prompt: false };
    renderSelfUpdate();
  } catch (failure) {
    el("self-update-text").textContent = String(failure);
  }
});

el("btn-retry").addEventListener("click", () => invoke("restart_app"));
el("btn-restart").addEventListener("click", () => invoke("restart_app"));

async function init() {
  const live = await attachListeners();

  // The worker may have finished before the listeners attached; the snapshot
  // closes that gap either way.
  try {
    applySnapshot(await invoke("get_state"));
  } catch (error) {
    shell = { phase: "failed", error: `无法与外壳通信：${error}` };
    renderFailure();
    return;
  }

  if (!live) startPolling();
  decide();
}

init();
