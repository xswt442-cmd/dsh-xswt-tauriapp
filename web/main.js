// Splash, failure and the update dialog — the shell's own page.
//
// The page is a small state machine. Tauri creates the bootstrap window on this
// page and a worker thread in Rust does the slow work; the page only decides
// *when* to hand over, so that an available update can be shown first. The
// hand-off happens exactly once, through `open_dsh` — and what it does is build
// a **separate** window for dsh. This page never navigates anywhere, which is
// the point: a shell page navigating to dsh would be a cross-site navigation,
// and dsh's `SameSite=Strict` session cookie would be withheld on it.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = (id) => document.getElementById(id);

/** Latest state snapshot pushed by the shell. */
let shell = null;
/** Latest update payload. */
let update = null;
/** Whether an update check has finished (successfully or not). */
let updateSettled = false;
/** Version currently highlighted in the dialog. */
let selected = null;
/** The version the "don't remind me" checkbox applies to. */
let dismissTarget = null;
/** The webview has been handed to the dsh UI. */
let navigated = false;
/** The update dialog is on screen. */
let dialogOpen = false;

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

function renderFailure() {
  el("splash").classList.add("hidden");
  el("failure").classList.remove("hidden");
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
    empty.textContent = "没有可用的版本信息。";
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

function showDialog() {
  dialogOpen = true;
  const report = update?.report;
  el("current-version").textContent = report?.current || shell?.current_version || "—";
  el("dsh-location").textContent = shell?.url || "—";

  if (update?.error) showInlineError(`更新检查失败：${update.error}`);
  else el("check-error").classList.add("hidden");

  // The popup is about the candidate — the newest version on a track at least
  // as stable as the installed one. That is also what the checkbox silences.
  dismissTarget = report?.candidate?.version || null;
  selected = dismissTarget;
  el("dismiss-target").textContent = dismissTarget || "—";
  el("dismiss-check").checked = false;

  renderChannels();
  updateButtons();
  el("dialog").classList.remove("hidden");
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
    navigated = false;
    dialogOpen = false;
    el("dialog").classList.add("hidden");
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
  if (state.update || state.update_error) {
    update = {
      current: state.current_version || "",
      report: state.update || null,
      error: state.update_error || null,
      should_prompt: Boolean(state.update?.candidate && !state.update?.candidate_dismissed),
    };
    updateSettled = true;
  }
}

/** Hand over as soon as the server is up and the update question is answered. */
function decide() {
  if (navigated || dialogOpen) return;
  if (!shell || shell.phase !== "ready") return;
  if (!updateSettled) return;
  if (update?.should_prompt) {
    showDialog();
    return;
  }
  goToDsh();
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
    ["shell://status", (event) => { shell = event.payload; renderSplash(); }],
    ["shell://ready", (event) => { shell = event.payload; decide(); }],
    ["shell://error", (event) => { shell = event.payload; renderFailure(); }],
    ["shell://update", (event) => { update = event.payload; updateSettled = true; decide(); }],
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

el("btn-later").addEventListener("click", async () => {
  if (el("dismiss-check").checked && dismissTarget) {
    try {
      await invoke("dismiss_version", { version: dismissTarget });
    } catch (error) {
      showInlineError(String(error));
      return;
    }
  }
  el("dialog").classList.add("hidden");
  goToDsh();
});

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
    const payload = await invoke("check_updates");
    update = payload;
    updateSettled = true;
    dismissTarget = payload.report?.candidate?.version || null;
    selected = dismissTarget;
    el("dismiss-target").textContent = dismissTarget || "—";
    if (payload.error || !payload.report) {
      renderChannels();
      updateButtons();
    } else {
      showDialog();
    }
    if (payload.error) showInlineError(`更新检查失败：${payload.error}`);
  } catch (error) {
    showInlineError(String(error));
  } finally {
    button.disabled = false;
    button.textContent = "重新检查";
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
