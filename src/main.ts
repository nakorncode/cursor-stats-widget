import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

type RecentChat = {
  conversationId: string;
  costUsd: number;
  startedMs: number;
  lastMs: number;
  eventCount: number;
};

type UsageSnapshot = {
  planPercentUsed: number;
  planRemainingPercent: number;
  membershipType: string;
  lastTaskCostUsd: number | null;
  lastTaskStartedMs: number | null;
  lastTaskLastMs: number | null;
  billingCycleEndMs: number | null;
  daysLeft: number;
  dailyBudgetPercent: number;
  todayUsedPercent: number;
  todayCostUsd: number;
  paceRatio: number;
  paceLabel: string;
  recentChats: RecentChat[];
  refreshedAtMs: number;
  error: string | null;
};

type Settings = {
  refreshSecs: number;
  recentChats: number;
  clockFormat: string;
  launchOnStartup?: boolean;
  compactMode: boolean;
};

const BACKOFF_MS = 60_000;
const BASE_HEIGHT = 188;
const COMPACT_WIDTH = 320;
const COMPACT_HEIGHT = 40;
const CHAT_ROW_HEIGHT = 18;
const CHATS_HEADER = 22;

let timer: number | undefined;
let inflight = false;
let lastSnap: UsageSnapshot | null = null;
let settings: Settings = {
  refreshSecs: 20,
  recentChats: 3,
  clockFormat: "system",
  compactMode: false,
};
let use12h = true;
let lastFittedKey = "";

function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} missing`);
  return el;
}

function localDayStartMs(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function fmtPct(n: number): string {
  return `${n.toFixed(n >= 10 ? 0 : 1)}%`;
}

function fmtUsd(n: number | null): string {
  if (n == null) return "—";
  if (n < 0.01 && n > 0) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(2)}`;
}

function resolveUse12h(clockFormat: string): boolean {
  if (clockFormat === "12h") return true;
  if (clockFormat === "24h") return false;
  try {
    const sample = new Intl.DateTimeFormat(undefined, { hour: "numeric" }).resolvedOptions();
    const cycle = (sample as { hourCycle?: string }).hourCycle;
    if (cycle === "h23" || cycle === "h24") return false;
    if (cycle === "h12" || cycle === "h11") return true;
    const probe = new Intl.DateTimeFormat(undefined, {
      hour: "numeric",
      minute: "2-digit",
    }).format(new Date(2024, 0, 1, 15, 0));
    return /am|pm/i.test(probe);
  } catch {
    return true;
  }
}

function fmtClock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
    hour12: use12h,
  });
}

function fmtRange(startedMs: number, lastMs: number): string {
  return `${fmtClock(startedMs)}–${fmtClock(lastMs)}`;
}

function fmtAge(ms: number): string {
  const sec = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (sec < 5) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 48) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

function paceText(snap: UsageSnapshot): string {
  const verdict =
    snap.paceLabel === "under"
      ? "Under"
      : snap.paceLabel === "over"
        ? "Over"
        : "On track";
  return `${verdict} · ${fmtPct(snap.todayUsedPercent)}/${fmtPct(snap.dailyBudgetPercent)} day · ${fmtUsd(snap.todayCostUsd)} today`;
}

function chatLine(c: RecentChat): string {
  return `${fmtUsd(c.costUsd)} · ${fmtRange(c.startedMs, c.lastMs)}`;
}

/** Fixed formula — never measure DOM (that caused endless height growth). */
async function fitWindow(chatCount: number) {
  const key = settings.compactMode ? "c" : `f${chatCount}`;
  if (key === lastFittedKey) return;
  lastFittedKey = key;
  try {
    if (settings.compactMode) {
      await getCurrentWindow().setSize(new LogicalSize(COMPACT_WIDTH, COMPACT_HEIGHT));
      return;
    }
    const extra = chatCount > 0 ? CHATS_HEADER + chatCount * CHAT_ROW_HEIGHT : 0;
    await getCurrentWindow().setSize(new LogicalSize(340, BASE_HEIGHT + extra));
  } catch {
    // ignore in browser preview
  }
}

function renderChats(snap: UsageSnapshot) {
  const section = $("chats-section");
  const list = $("chat-list");
  const chats = snap.recentChats ?? [];
  const show =
    !settings.compactMode &&
    settings.recentChats > 0 &&
    !snap.error &&
    chats.length > 0;

  section.classList.toggle("hidden", !show);
  list.innerHTML = "";
  if (!show) {
    void fitWindow(0);
    return;
  }

  for (const c of chats) {
    const row = document.createElement("div");
    row.className = "chat-row";
    row.textContent = chatLine(c);
    list.appendChild(row);
  }
  void fitWindow(chats.length);
}

function render(snap: UsageSnapshot) {
  lastSnap = snap;
  const panel = document.querySelector(".panel");
  panel?.classList.toggle("compact", settings.compactMode);
  panel?.classList.toggle("error", Boolean(snap.error));
  panel?.classList.remove("pace-under", "pace-on", "pace-over");
  if (!snap.error) {
    panel?.classList.add(
      snap.paceLabel === "under"
        ? "pace-under"
        : snap.paceLabel === "over"
          ? "pace-over"
          : "pace-on",
    );
  }

  $("membership").textContent = snap.membershipType
    ? snap.membershipType.replace(/_/g, " ")
    : "";
  $("plan-used").textContent = snap.error ? "—" : fmtPct(snap.planPercentUsed);

  if (snap.error || snap.lastTaskCostUsd == null) {
    $("last-cost").textContent = "—";
  } else if (snap.lastTaskStartedMs != null && snap.lastTaskLastMs != null) {
    $("last-cost").textContent = `${fmtUsd(snap.lastTaskCostUsd)} · ${fmtRange(
      snap.lastTaskStartedMs,
      snap.lastTaskLastMs,
    )}`;
  } else {
    $("last-cost").textContent = fmtUsd(snap.lastTaskCostUsd);
  }

  $("period-left").textContent = snap.error ? "—" : fmtPct(snap.planRemainingPercent);
  $("pace").textContent = snap.error ? "—" : paceText(snap);
  renderChats(snap);
  updateStatus();
}

function updateStatus() {
  if (!lastSnap || settings.compactMode) return;
  $("status").textContent = lastSnap.error
    ? lastSnap.error
    : `Updated ${fmtAge(lastSnap.refreshedAtMs)}`;
}

async function refresh(bypassCache = false) {
  if (inflight) return;
  inflight = true;
  try {
    const snap = await invoke<UsageSnapshot>("get_usage", {
      dayStartMs: localDayStartMs(),
      bypassCache,
    });
    render(snap);
    if (snap.error) scheduleBackoff();
    else scheduleNext();
  } catch (e) {
    render({
      planPercentUsed: 0,
      planRemainingPercent: 0,
      membershipType: "",
      lastTaskCostUsd: null,
      lastTaskStartedMs: null,
      lastTaskLastMs: null,
      billingCycleEndMs: null,
      daysLeft: 0,
      dailyBudgetPercent: 0,
      todayUsedPercent: 0,
      todayCostUsd: 0,
      paceRatio: 0,
      paceLabel: "",
      recentChats: [],
      refreshedAtMs: Date.now(),
      error: String(e),
    });
    scheduleBackoff();
  } finally {
    inflight = false;
  }
}

function clearRefreshTimer() {
  if (timer) window.clearTimeout(timer);
  timer = undefined;
}

function scheduleNext() {
  clearRefreshTimer();
  if (settings.refreshSecs < 0) return;
  timer = window.setTimeout(() => {
    void refresh(false);
  }, settings.refreshSecs * 1000);
}

function scheduleBackoff() {
  clearRefreshTimer();
  timer = window.setTimeout(() => {
    void refresh(false);
  }, BACKOFF_MS);
}

function applySettings(s: Settings) {
  const modeChanged = Boolean(s.compactMode) !== settings.compactMode;
  const chatsChanged = s.recentChats !== settings.recentChats;
  settings = {
    ...s,
    compactMode: Boolean(s.compactMode),
  };
  use12h = resolveUse12h(s.clockFormat);
  if (modeChanged || chatsChanged) lastFittedKey = ""; // force resize
  if (lastSnap) render(lastSnap);
  else void fitWindow(0);
  scheduleNext();
}

async function toggleChartWindow() {
  const existing = await WebviewWindow.getByLabel("chart");
  if (existing) {
    if (await existing.isVisible()) {
      await existing.hide();
    } else {
      await existing.show();
      await existing.setFocus();
    }
    return;
  }

  const chart = new WebviewWindow("chart", {
    url: "chart.html",
    title: "Cursor cost chart",
    width: 520,
    height: 340,
    resizable: true,
    decorations: false,
    transparent: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    visible: true,
  });

  chart.once("tauri://error", (e) => {
    console.error("chart window error", e);
    $("status").textContent = `Chart failed: ${String(e.payload ?? e)}`;
  });
}

/**
 * Move the overlay with setPosition instead of startDragging / data-tauri-drag-region.
 * Native Win32 drag activates Windows snap / half-screen split near edges.
 */
function installSnapSafeDrag(root: HTMLElement) {
  const win = getCurrentWindow();
  type Drag = { ox: number; oy: number; sx: number; sy: number; ready: boolean };
  let drag: Drag | null = null;

  const onMove = (e: PointerEvent) => {
    if (!drag?.ready) return;
    const { ox, oy, sx, sy } = drag;
    void win.setPosition(new LogicalPosition(ox + (e.screenX - sx), oy + (e.screenY - sy)));
  };

  const endDrag = (e: PointerEvent) => {
    if (!drag) return;
    drag = null;
    try {
      root.releasePointerCapture(e.pointerId);
    } catch {
      // ignore
    }
  };

  root.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    if (!(e.target instanceof Element)) return;
    if (e.target.closest("button, a, input, textarea, select, [data-no-drag]")) return;

    const sx = e.screenX;
    const sy = e.screenY;
    const token: Drag = { ox: 0, oy: 0, sx, sy, ready: false };
    drag = token;
    root.setPointerCapture(e.pointerId);

    void (async () => {
      try {
        const scale = await win.scaleFactor();
        const phys = await win.outerPosition();
        const logical = phys.toLogical(scale);
        if (drag !== token) return;
        token.ox = logical.x;
        token.oy = logical.y;
        token.ready = true;
      } catch {
        if (drag === token) drag = null;
      }
    })();
  });

  root.addEventListener("pointermove", onMove);
  root.addEventListener("pointerup", endDrag);
  root.addEventListener("pointercancel", endDrag);
}

window.addEventListener("DOMContentLoaded", () => {
  const panel = document.querySelector(".panel");
  if (panel instanceof HTMLElement) installSnapSafeDrag(panel);

  $("refresh-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    void refresh(true);
  });
  $("chart-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    void toggleChartWindow();
  });
  $("close-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    void invoke("hide_overlay");
  });
  void listen<boolean>("refresh-usage", (ev) => {
    void refresh(Boolean(ev.payload));
  });
  void listen("toggle-chart", () => {
    void toggleChartWindow();
  });
  void listen<Settings>("settings-changed", (ev) => {
    applySettings(ev.payload);
  });
  window.setInterval(updateStatus, 1000);
  void invoke<Settings>("get_settings")
    .then(applySettings)
    .finally(() => {
      void refresh(false);
    });
});
