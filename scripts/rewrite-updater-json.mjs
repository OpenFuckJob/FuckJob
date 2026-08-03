import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const API_ASSET_URL =
  /^https:\/\/api\.github\.com\/repos\/[^/]+\/[^/]+\/releases\/assets\/(?<id>\d+)$/;

function publicAssetUrl(asset, releaseTag) {
  const draftUrl = new URL(asset.browser_download_url);
  const downloadMarker = "/releases/download/";
  const markerIndex = draftUrl.pathname.indexOf(downloadMarker);
  if (markerIndex < 0 || !asset.name) {
    throw new Error(`Release 资产地址无效：${asset.browser_download_url}`);
  }
  const repositoryPath = draftUrl.pathname.slice(0, markerIndex);
  return `${draftUrl.origin}${repositoryPath}${downloadMarker}${encodeURIComponent(releaseTag)}/${encodeURIComponent(asset.name)}`;
}

export function rewriteUpdaterUrls(updater, release) {
  if (!updater?.platforms || typeof updater.platforms !== "object") {
    throw new Error("latest.json 缺少 platforms");
  }
  if (!release?.tag_name || !Array.isArray(release.assets)) {
    throw new Error("Release 元数据不完整");
  }

  const assetsById = new Map();
  const rewrittenUrls = new Map();
  for (const asset of release.assets) {
    const publicUrl = publicAssetUrl(asset, release.tag_name);
    assetsById.set(String(asset.id), publicUrl);
    rewrittenUrls.set(asset.browser_download_url, publicUrl);
    if (asset.url) rewrittenUrls.set(asset.url, publicUrl);
  }
  const publicAssetUrls = new Set(assetsById.values());
  const expectedReleasePath = `/releases/download/${release.tag_name}/`;
  const rewritten = structuredClone(updater);

  if (typeof release.body === "string" && release.body.trim()) {
    rewritten.notes = release.body.trim();
  }

  for (const [platform, target] of Object.entries(rewritten.platforms)) {
    if (!target?.url) throw new Error(`${platform} 缺少下载地址`);
    const match = target.url.match(API_ASSET_URL);
    if (match) {
      const publicUrl = assetsById.get(match.groups.id);
      if (!publicUrl) {
        throw new Error(`${platform} 引用了不存在的 Release 资产 ${match.groups.id}`);
      }
      target.url = publicUrl;
    } else if (rewrittenUrls.has(target.url)) {
      target.url = rewrittenUrls.get(target.url);
    }
    if (
      !publicAssetUrls.has(target.url) ||
      !target.url.startsWith("https://github.com/") ||
      !target.url.includes(expectedReleasePath)
    ) {
      throw new Error(`${platform} 不是公开 Release 下载地址：${target.url}`);
    }
  }

  return rewritten;
}

async function main() {
  const [updaterPath, releasePath] = process.argv.slice(2);
  if (!updaterPath || !releasePath) {
    throw new Error("用法：node scripts/rewrite-updater-json.mjs <latest.json> <release.json>");
  }
  const updater = JSON.parse(await readFile(updaterPath, "utf8"));
  const release = JSON.parse(await readFile(releasePath, "utf8"));
  const rewritten = rewriteUpdaterUrls(updater, release);
  await writeFile(updaterPath, `${JSON.stringify(rewritten, null, 2)}\n`, "utf8");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
