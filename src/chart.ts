import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type CostBucket = {
  startMs: number;
  endMs: number;
  costUsd: number;
  eventCount: number;
};

type CostSeries = {
  rangeSecs: number;
  bucketSecs: number;
  totalUsd: number;
  buckets: CostBucket[];
  error: string | null;
};

type BarHit = {
  x: number;
  y: number;
  w: number;
  h: number;
  bucket: CostBucket;
};

let rangeSecs = 3600;
let inflight = false;
let lastSeries: CostSeries | null = null;
let barHits: BarHit[] = [];
let hoverIndex = -1;

function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} missing`);
  return el;
}

function fmtUsd(n: number): string {
  if (n <= 0) return "$0";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 1) return `$${n.toFixed(3)}`;
  return `$${n.toFixed(2)}`;
}

/** Compact label that fits inside thin bars. */
function fmtUsdShort(n: number): string {
  if (n <= 0) return "";
  if (n < 0.01) return n.toFixed(3);
  if (n < 1) return n.toFixed(2);
  if (n < 10) return n.toFixed(2);
  return n.toFixed(1);
}

function fmtClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function fmtRange(b: CostBucket): string {
  return `${fmtClock(b.startMs)} – ${fmtClock(b.endMs)}`;
}

function hideTooltip() {
  const tip = $("chart-tooltip");
  tip.classList.add("hidden");
  tip.textContent = "";
}

function showTooltip(hit: BarHit, clientX: number, clientY: number) {
  const tip = $("chart-tooltip");
  const panel = document.querySelector(".chart-panel") as HTMLElement;
  const rect = panel.getBoundingClientRect();
  tip.classList.remove("hidden");
  tip.innerHTML = `<strong>${fmtUsd(hit.bucket.costUsd)}</strong><br>${fmtRange(hit.bucket)}<br><span class="tip-muted">${hit.bucket.eventCount} event${hit.bucket.eventCount === 1 ? "" : "s"}</span>`;

  const tw = tip.offsetWidth;
  const th = tip.offsetHeight;
  let left = clientX - rect.left + 12;
  let top = clientY - rect.top - th - 8;
  if (left + tw > rect.width - 8) left = rect.width - tw - 8;
  if (left < 8) left = 8;
  if (top < 8) top = clientY - rect.top + 16;
  tip.style.left = `${left}px`;
  tip.style.top = `${top}px`;
}

function draw(series: CostSeries, highlight = -1) {
  const canvas = document.getElementById("chart") as HTMLCanvasElement;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const dpr = window.devicePixelRatio || 1;
  const cssW = Math.max(canvas.clientWidth || 480, 320);
  const cssH = Math.max(canvas.clientHeight || 220, 180);
  canvas.width = Math.floor(cssW * dpr);
  canvas.height = Math.floor(cssH * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  ctx.clearRect(0, 0, cssW, cssH);
  const pad = { t: 14, r: 8, b: 28, l: 40 };
  const w = cssW - pad.l - pad.r;
  const h = cssH - pad.t - pad.b;
  const max = Math.max(...series.buckets.map((b) => b.costUsd), 0.01);
  const n = Math.max(series.buckets.length, 1);
  const gap = Math.max(1, Math.min(3, Math.floor(w / n / 8)));
  const barW = Math.max(2, (w - gap * (n - 1)) / n);

  ctx.strokeStyle = "rgba(255,255,255,0.08)";
  ctx.beginPath();
  ctx.moveTo(pad.l, pad.t);
  ctx.lineTo(pad.l, pad.t + h);
  ctx.lineTo(pad.l + w, pad.t + h);
  ctx.stroke();

  ctx.fillStyle = "rgba(232,234,237,0.45)";
  ctx.font = "11px Segoe UI, system-ui, sans-serif";
  ctx.textAlign = "right";
  ctx.fillText(fmtUsd(max), pad.l - 6, pad.t + 10);
  ctx.fillText("$0", pad.l - 6, pad.t + h);

  barHits = [];

  series.buckets.forEach((b, i) => {
    const x = pad.l + i * (barW + gap);
    const bh = Math.max(b.costUsd > 0 ? 2 : 0, (b.costUsd / max) * h);
    const y = pad.t + h - bh;
    const active = i === highlight;
    ctx.fillStyle =
      b.costUsd > 0
        ? active
          ? "#99f6e4"
          : "#5eead4"
        : "rgba(255,255,255,0.06)";
    ctx.fillRect(x, y, barW, bh);
    barHits.push({ x, y, w: barW, h: Math.max(bh, 8), bucket: b });

    // In-bar amount label (skip empty / too-narrow bars)
    if (b.costUsd > 0 && barW >= 10) {
      const label = fmtUsdShort(b.costUsd);
      const fontPx = barW >= 22 ? 10 : 8;
      ctx.font = `600 ${fontPx}px Segoe UI, system-ui, sans-serif`;
      ctx.textAlign = "center";
      const tx = x + barW / 2;
      if (bh >= fontPx + 6) {
        // Inside bar, near top
        ctx.fillStyle = "#0f172a";
        ctx.textBaseline = "top";
        ctx.fillText(label, tx, y + 2);
      } else {
        // Above bar when too short
        ctx.fillStyle = "rgba(232,234,237,0.85)";
        ctx.textBaseline = "bottom";
        ctx.fillText(label, tx, y - 1);
      }
    }
  });

  ctx.fillStyle = "rgba(232,234,237,0.45)";
  ctx.font = "11px Segoe UI, system-ui, sans-serif";
  ctx.textBaseline = "alphabetic";
  ctx.textAlign = "left";
  const first = series.buckets[0];
  const last = series.buckets[series.buckets.length - 1];
  if (first) {
    ctx.fillText(fmtClock(first.startMs), pad.l, cssH - 8);
  }
  if (last) {
    ctx.textAlign = "right";
    ctx.fillText(fmtClock(last.endMs), pad.l + w, cssH - 8);
  }
}

function hitTest(cssX: number): number {
  for (let i = 0; i < barHits.length; i++) {
    const b = barHits[i];
    // Full column hit area is easier to hover than just the filled bar
    if (cssX >= b.x && cssX <= b.x + b.w) {
      return i;
    }
  }
  return -1;
}

function onCanvasMove(ev: MouseEvent) {
  if (!lastSeries) return;
  const canvas = document.getElementById("chart") as HTMLCanvasElement;
  const rect = canvas.getBoundingClientRect();
  const x = ev.clientX - rect.left;
  const idx = hitTest(x);
  if (idx !== hoverIndex) {
    hoverIndex = idx;
    draw(lastSeries, hoverIndex);
  }
  if (idx >= 0) {
    showTooltip(barHits[idx], ev.clientX, ev.clientY);
  } else {
    hideTooltip();
  }
}

function onCanvasLeave() {
  if (!lastSeries) return;
  hoverIndex = -1;
  hideTooltip();
  draw(lastSeries, -1);
}

async function load() {
  if (inflight) return;
  inflight = true;
  hideTooltip();
  hoverIndex = -1;
  $("chart-status").textContent = "Loading…";
  try {
    const series = await invoke<CostSeries>("get_cost_series", {
      rangeSecs,
      bypassCache: false,
    });
    if (series.error) {
      lastSeries = null;
      barHits = [];
      $("chart-total").textContent = "—";
      $("chart-status").textContent = series.error;
      draw({ ...series, buckets: [] });
    } else {
      lastSeries = series;
      $("chart-total").textContent = `Total ${fmtUsd(series.totalUsd)}`;
      const events = series.buckets.reduce((a, b) => a + b.eventCount, 0);
      $("chart-status").textContent = `${Math.round(series.bucketSecs / 60)}m bars · ${events} events`;
      requestAnimationFrame(() => draw(series));
    }
  } catch (e) {
    $("chart-status").textContent = String(e);
  } finally {
    inflight = false;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  $("close-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    void getCurrentWindow().hide();
  });

  document.querySelectorAll<HTMLButtonElement>("#ranges button").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      document.querySelectorAll("#ranges button").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      rangeSecs = Number(btn.dataset.range || 3600);
      void load();
    });
  });

  const canvas = document.getElementById("chart") as HTMLCanvasElement;
  canvas.addEventListener("mousemove", onCanvasMove);
  canvas.addEventListener("mouseleave", onCanvasLeave);

  void listen("refresh-chart", () => {
    void load();
  });
  window.addEventListener("resize", () => {
    if (lastSeries) requestAnimationFrame(() => draw(lastSeries!, hoverIndex));
  });
  void load();
});
