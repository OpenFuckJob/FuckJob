import { readFile, readdir } from "node:fs/promises";
import { extname, join, relative } from "node:path";

const root = process.cwd();
const roots = ["src", "src-tauri/src", "src-tauri/capabilities", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "package.json", "build.sh"];
const ignored = /(?:^|\/)(?:docs|target|dist|node_modules)(?:\/|$)|(?:\.test\.|\.spec\.)|(?:^|\/)check-network-boundary\.mjs$|(?:pnpm-lock|Cargo\.lock)/;
const textExtensions = new Set([".ts", ".tsx", ".js", ".mjs", ".rs", ".json", ".yaml", ".yml", ".toml", ".sh", ""]);
const forbidden = [
  ["legacy server host", /fk\.pgthinker\.me/i],
  ["legacy API prefix", /\/prod-api\b/i],
  ["legacy auth endpoint", /\/(?:api\/)?auth\/(?:login|register|session|logout)\b/i],
  ["legacy points endpoint", /\/(?:api\/)?points(?:\/|\b)/i],
  ["legacy RPA generation endpoint", /\/rpa\/generate(?:\/|\b)/i],
  ["hardcoded DashScope search", /enable_search|dashscope[^\n]{0,100}(?:search|联网搜索)/i],
  ["updater runtime/config", /tauri[-_]plugin[-_]updater|plugin-updater|createUpdaterArtifacts|generate_update_json|update\.json/i],
];

async function filesAt(path) {
  const full = join(root, path);
  try {
    const entries = await readdir(full, { withFileTypes: true });
    return (await Promise.all(entries.map((entry) => filesAt(join(path, entry.name))))).flat();
  } catch {
    return [path];
  }
}

const files = (await Promise.all(roots.map(filesAt))).flat().filter((file) => !ignored.test(file) && textExtensions.has(extname(file)));
const violations = [];
for (const file of files) {
  const content = await readFile(join(root, file), "utf8");
  for (const [label, pattern] of forbidden) if (pattern.test(content)) violations.push(`${relative(root, join(root, file))}: ${label}`);
}
if (violations.length) {
  console.error(`Runtime network boundary violations:\n${violations.join("\n")}`);
  process.exit(1);
}
console.log(`Network boundary OK (${files.length} runtime files scanned).`);
