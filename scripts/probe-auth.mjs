import { execFileSync } from "node:child_process";
import { join } from "node:path";

const db = join(process.env.APPDATA, "Cursor", "User", "globalStorage", "state.vscdb");
const tok = execFileSync("sqlite3", [db, "SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken';"], {
  encoding: "utf8",
}).trim();

console.log("len", tok.length, "eyJ", tok.startsWith("eyJ"));
const p = tok.split(".")[1];
const json = JSON.parse(Buffer.from(p.replace(/-/g, "+").replace(/_/g, "/"), "base64").toString());
console.log("sub", json.sub);
console.log("type", json.type);
console.log("aud", json.aud);
console.log("exp", json.exp, new Date(json.exp * 1000).toISOString());

const cookieVal = `${json.sub}::${tok}`;
const headers = { Cookie: `WorkosCursorSessionToken=${cookieVal}` };

const summaryRes = await fetch("https://cursor.com/api/usage-summary", { headers });
console.log("usage-summary", summaryRes.status);
const summaryText = await summaryRes.text();
console.log(summaryText.slice(0, 800));

const meRes = await fetch("https://cursor.com/api/auth/me", { headers });
console.log("auth/me", meRes.status);
console.log((await meRes.text()).slice(0, 400));

const eventsRes = await fetch("https://cursor.com/api/dashboard/get-filtered-usage-events", {
  method: "POST",
  headers: {
    ...headers,
    "Content-Type": "application/json",
    Origin: "https://cursor.com",
  },
  body: JSON.stringify({ page: 1, pageSize: 1 }),
});
console.log("events", eventsRes.status);
console.log((await eventsRes.text()).slice(0, 800));
