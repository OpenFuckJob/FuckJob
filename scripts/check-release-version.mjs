import { readFile } from "node:fs/promises";

const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;
if (!tag || !/^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(tag)) {
  console.error(`发布标签必须使用 v<semver> 格式，当前值：${tag ?? "<empty>"}`);
  process.exit(1);
}

const expected = tag.slice(1);
const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const tauriConfig = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const cargoVersion = cargoToml.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];

const versions = {
  "package.json": packageJson.version,
  "src-tauri/tauri.conf.json": tauriConfig.version,
  "src-tauri/Cargo.toml": cargoVersion,
};
const mismatches = Object.entries(versions).filter(([, version]) => version !== expected);

if (mismatches.length > 0) {
  console.error(`标签 ${tag} 与应用版本不一致：`);
  for (const [file, version] of Object.entries(versions)) {
    console.error(`- ${file}: ${version ?? "<missing>"}`);
  }
  process.exit(1);
}

console.log(`发布版本校验通过：${tag}`);
