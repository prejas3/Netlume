// i18n check: every PL string key has an EN key (and back), and no Polish text sits outside the
// PL dictionary in src/main.ts or the "pl" arms of tray_texts in src-tauri/src/lib.rs.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const polish = /[ąćęłńóśźżĄĆĘŁŃÓŚŹŻ]/;
const problems = [];

const main = readFileSync(join(root, "src/main.ts"), "utf8");
const block = (start) => {
  const from = main.indexOf(start);
  if (from < 0) throw new Error(`missing ${start}`);
  return { from, to: main.indexOf("\n};", from) };
};
const en = block("const en = {"), pl = block("const pl: typeof en = {");
const keys = ({ from, to }) => new Set([...main.slice(from, to).matchAll(/(?:^|[{,]\s*)([a-zA-Z]\w*):\s/gm)].map((m) => m[1]));
const enKeys = keys(en), plKeys = keys(pl);
for (const k of plKeys) if (!enKeys.has(k)) problems.push(`PL key without EN: ${k}`);
for (const k of enKeys) if (!plKeys.has(k)) problems.push(`EN key without PL: ${k}`);

const files = (dir, ext) => readdirSync(join(root, dir), { recursive: true })
  .filter((f) => ext.test(f)).map((f) => join(dir, f));
for (const file of [...files("src", /\.(ts|js|css|html)$/), ...files("src-tauri/src", /\.rs$/)]) {
  const text = readFileSync(join(root, file), "utf8");
  let offset = 0;
  text.split("\n").forEach((line, i) => {
    const at = offset; offset += line.length + 1;
    if (!polish.test(line)) return;
    const inPl = file.endsWith("main.ts") && at > pl.from && at < pl.to;
    const trayPl = file.endsWith("lib.rs") && /\("pl", (true|false)\)|tray_texts\("pl"/.test(line);
    if (!inPl && !trayPl) problems.push(`Polish text outside the PL strings: ${file}:${i + 1}: ${line.trim()}`);
  });
}

if (problems.length) { console.error(problems.join("\n")); process.exit(1); }
console.log(`i18n ok: ${enKeys.size} keys in PL and EN, no stray Polish text`);
