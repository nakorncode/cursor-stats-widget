# Cursor Stats

Windows always-on-top overlay + system tray widget for **personal Cursor usage**.

Shows plan usage, last conversation cost, period remaining, today’s pace, recent chats, and a cost chart — using the same session your Cursor app already has.

![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)
![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6.svg)

> **Unofficial.** Reads your local Cursor session and calls Cursor’s undocumented dashboard APIs. Those endpoints can change without notice. Token stays on your machine; requests go only to `cursor.com`.

## Features

- **Overlay** — plan usage %, last task cost (summed by conversation), period left (`% · Nd · 29 Aug`), today’s pace (`used%/budget%/day · $ today`)
- **Compact mode** — tray toggle; pace text only + hide button
- **Recent chats** — last 24h, grouped by conversation; tray count **1 / 2 / 3 / 5 / 10 / Hide**
- **Cost chart** — 5m → 3d ranges, in-bar `$` labels, hover tooltip (amount + time range)
- **Tray** — show/hide, refresh interval (incl. never), clock format, compact mode, Launch on startup
- **Auth** — auto-reads `%APPDATA%\Cursor\User\globalStorage\state.vscdb` (`cursorAuth/accessToken`); optional `CURSOR_SESSION_TOKEN`

## Install (Windows)

Download the latest release from **[Releases](https://github.com/nakorncode/cursor-stats-widget/releases)**:

| Asset | Use |
|-------|-----|
| `Cursor Stats_x.y.z_x64-setup.exe` | NSIS installer (recommended) |
| `Cursor Stats_x.y.z_x64_en-US.msi` | MSI installer |
| `cursor-stats-widget.exe` | Portable executable (if attached) |

Sign in to the **Cursor desktop app** at least once so a local session exists, then launch Cursor Stats.

## Develop

Requirements: Windows, [Node.js](https://nodejs.org/) 20+, [pnpm](https://pnpm.io/), [Rust](https://rustup.rs/).

```powershell
git clone https://github.com/nakorncode/cursor-stats-widget.git
cd cursor-stats-widget
pnpm install
pnpm tauri:dev
```

Production build (exe + installers):

```powershell
pnpm tauri:build
```

Outputs under `src-tauri/target/release/bundle/`.

## Tray settings

| Menu | Options |
|------|---------|
| Refresh every | 10s … 10m, or Never (manual) |
| Recent chats | 1 / 2 / 3 / 5 / 10 / Hide |
| Clock format | System (fallback 12h) / 12-hour / 24-hour |
| **Compact mode** | Pace-only strip (tray checkbox; default off) |
| **Launch on startup** | On by default; toggle from tray checkbox |

Left-click tray icon toggles the overlay. Right-click opens the menu.

Drag the overlay body to move it (buttons excluded). Movement uses physical `cursorPosition` + `setPosition` so Windows snap stays off and mixed-DPI / multi-monitor moves stay stable.

## Recent chats (cost model)

Cursor reuses one `conversationId` for a long-lived chat. We **split sessions** when idle time between events exceeds **30 minutes**, so “recent #1” is the latest active burst — not the whole day’s total for that id.

## Branch / release

See [docs/BRANCHING.md](./docs/BRANCHING.md): work on `develop` (rebase onto `main` first); **always merge to `main` before tagging** a release.

## Today's pace

```
daysLeft            = ceil((billingCycleEnd − localDayStart) ÷ 24h)   # at least 1
remainingAtDayStart = planRemaining% + todayUsed%
dailyBudget%        = remainingAtDayStart ÷ daysLeft
todayUsed%          = todayCost¢ ÷ (plan.used ÷ totalPercentUsed) × 100
pace                = todayUsed% ÷ dailyBudget%
```

Uses **calendar days from local midnight**, not raw hours÷24 — so “2 days left” with 18% remaining budgets **9%/day**, not 18%.

`todayUsed%` is in the **same units as period-left** (`totalPercentUsed`). On current Pro, `plan.used` is cents (not request units) and `breakdown.total` is consumed amount, not pool size — the pool is inferred as `used / totalPercentUsed` (included-total, ~$345 on Pro). Using `requestsCosts / used` made X% jump after a few chats on a fresh Pro cycle.

`dailyBudget%` is based on remaining **at day start** (live remaining + today’s usage), so spending during the day does not shrink the allowance (avoids 9% → 7.2% drift).

| Ratio | Label |
|-------|--------|
| &lt; 0.7 | Under |
| 0.7–1.3 | On track |
| &gt; 1.3 | Over |

Display example: `On track · 3.1%/8.6% day · $1.24 today`

## Privacy

- Session token is read locally (or from `CURSOR_SESSION_TOKEN`).
- Network calls only to `https://cursor.com` usage/dashboard endpoints.
- No telemetry from this app.

## Stack

- [Tauri 2](https://tauri.app/) + vanilla TypeScript + Rust
- `rusqlite` (bundled) · `reqwest` · canvas chart (no chart library)

## Releases / CI

Push a version tag to build Windows installers and publish a GitHub Release:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Workflow: `.github/workflows/release.yml` (Windows · NSIS + MSI via `tauri-action`).

## License

[MIT](./LICENSE) © nakorncode
