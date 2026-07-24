import { execFileSync } from "node:child_process";
import { join } from "node:path";

const db = join(process.env.APPDATA, "Cursor", "User", "globalStorage", "state.vscdb");
const tok = execFileSync(
  "sqlite3",
  [db, "SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken';"],
  { encoding: "utf8" },
).trim();
const payload = JSON.parse(
  Buffer.from(tok.split(".")[1].replace(/-/g, "+").replace(/_/g, "/"), "base64").toString(),
);
const cookie = `${payload.sub}::${tok}`;
const end = Date.now();
const start = end - 24 * 3600_000;

const res = await fetch("https://cursor.com/api/dashboard/get-filtered-usage-events", {
  method: "POST",
  headers: {
    Cookie: `WorkosCursorSessionToken=${cookie}`,
    Origin: "https://cursor.com",
    "Content-Type": "application/json",
  },
  body: JSON.stringify({
    page: 1,
    pageSize: 100,
    startDate: String(start),
    endDate: String(end),
  }),
});
const data = await res.json();
const events = data.usageEventsDisplay || [];
console.log("fetched", events.length, "of", data.totalUsageEventsCount);

const byConv = new Map();
for (const e of events) {
  const id = e.conversationId && e.conversationId !== "null" ? e.conversationId : `event:${e.timestamp}`;
  const cents = e.chargedCents ?? e.tokenUsage?.totalCents ?? 0;
  const ts = Number(e.timestamp);
  if (!byConv.has(id)) byConv.set(id, []);
  byConv.get(id).push({ ts, cents, model: e.model });
}

const ranked = [...byConv.entries()]
  .map(([id, list]) => {
    list.sort((a, b) => a.ts - b.ts);
    const gaps = [];
    for (let i = 1; i < list.length; i++) gaps.push((list[i].ts - list[i - 1].ts) / 60000);
    return {
      id: id.slice(0, 12),
      n: list.length,
      usd: list.reduce((s, x) => s + x.cents, 0) / 100,
      spanMin: (list[list.length - 1].ts - list[0].ts) / 60000,
      maxGapMin: gaps.length ? Math.max(...gaps) : 0,
      last: new Date(list[list.length - 1].ts).toISOString(),
    };
  })
  .sort((a, b) => (b.last > a.last ? 1 : -1));

console.log("top conversations by last activity:");
console.table(ranked.slice(0, 8));

// Simulate 30m session split on newest conv
const newest = [...byConv.entries()].sort((a, b) => {
  const la = Math.max(...a[1].map((x) => x.ts));
  const lb = Math.max(...b[1].map((x) => x.ts));
  return lb - la;
})[0];
const GAP = 30 * 60_000;
const list = newest[1].sort((a, b) => a.ts - b.ts);
const sessions = [];
let cur = [list[0]];
for (let i = 1; i < list.length; i++) {
  if (list[i].ts - cur[cur.length - 1].ts > GAP) {
    sessions.push(cur);
    cur = [list[i]];
  } else cur.push(list[i]);
}
sessions.push(cur);
console.log("newest conv", newest[0].slice(0, 12), "fullDayUsd", list.reduce((s, x) => s + x.cents, 0) / 100);
console.log(
  "sessions@30m",
  sessions.map((s) => ({
    n: s.length,
    usd: s.reduce((a, x) => a + x.cents, 0) / 100,
    spanMin: (s[s.length - 1].ts - s[0].ts) / 60000,
  })),
);
