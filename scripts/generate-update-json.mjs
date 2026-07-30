import { readFile, readdir, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";

const version = process.argv[2];
if (!version) {
  console.error("用法: pnpm update:manifest -- <version> [更新说明]");
  process.exit(1);
}

const notes = process.argv[3] ?? `更新版本 ${version}`;
const releaseDir = join("releases", version);
const baseUrl = (process.env.UPDATE_BASE_URL ?? "https://fk.pgthinker.me/releases").replace(/\/$/, "");
const platforms = {};

async function addArtifacts(directory, platformName, artifactPattern) {
  const path = join(releaseDir, directory);
  let files;
  try {
    files = await readdir(path);
  } catch {
    return;
  }

  for (const signatureFile of files.filter((file) => file.endsWith(".sig"))) {
    const artifact = signatureFile.slice(0, -4);
    if (!artifactPattern.test(artifact)) continue;
    const signature = (await readFile(join(path, signatureFile), "utf8")).trim();
    platforms[platformName(artifact)] = {
      url: `${baseUrl}/${version}/${directory}/${encodeURIComponent(basename(artifact))}`,
      signature,
    };
  }
}

await addArtifacts(
  "darwin",
  (name) => `darwin-${/aarch64|arm64/i.test(name) ? "aarch64" : "x86_64"}`,
  /\.app\.tar\.gz$/,
);
await addArtifacts(
  "windows",
  (name) => `windows-${/aarch64|arm64/i.test(name) ? "aarch64" : "x86_64"}`,
  /\.exe$/,
);

if (Object.keys(platforms).length === 0) {
  console.error(`在 ${releaseDir} 中没有找到带 .sig 的更新产物`);
  process.exit(1);
}

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms,
};
const output = join(releaseDir, "update.json");
await writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
console.log(`已生成 ${output}，包含 ${Object.keys(platforms).join(", ")}`);
