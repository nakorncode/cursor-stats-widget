/**
 * Structural assert: snap-safe drag must use PhysicalPosition + cursorPosition
 * (LogicalPosition + screenX flickered across mixed-DPI monitors).
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const indexHtml = fs.readFileSync(path.join(root, "index.html"), "utf8");
const mainTs = fs.readFileSync(path.join(root, "src/main.ts"), "utf8");

const failures = [];

if (/data-tauri-drag-region/.test(indexHtml)) {
  failures.push("index.html still has data-tauri-drag-region (native drag → Windows snap)");
}
if (!/function installSnapSafeDrag/.test(mainTs)) {
  failures.push("src/main.ts missing installSnapSafeDrag");
}
if (!/cursorPosition\(/.test(mainTs)) {
  failures.push("src/main.ts missing cursorPosition() (needed for multi-monitor drag)");
}
if (!/setPosition\(new PhysicalPosition/.test(mainTs)) {
  failures.push("src/main.ts missing setPosition(PhysicalPosition) drag path");
}
if (/setPosition\(new LogicalPosition/.test(mainTs)) {
  failures.push("src/main.ts still uses LogicalPosition for drag (mixed-DPI flicker)");
}
if (/startDragging\(/.test(mainTs)) {
  failures.push("src/main.ts calls startDragging (would re-enable Windows snap)");
}

if (failures.length) {
  console.error("assert-snap-safe-drag FAILED:");
  for (const f of failures) console.error(" -", f);
  process.exit(1);
}

console.log("assert-snap-safe-drag OK");
