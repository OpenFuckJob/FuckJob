import assert from "node:assert/strict";
import test from "node:test";
import { rewriteUpdaterUrls } from "./rewrite-updater-json.mjs";

const release = {
  tag_name: "v0.1.6",
  body: "本版本新增模拟面试异步报告。",
  repository: { full_name: "OpenFuckJob/FuckJob" },
  assets: [
    {
      id: 123,
      name: "OfferFlow_0.1.6_x64-setup.exe",
      url: "https://api.github.com/repos/OpenFuckJob/FuckJob/releases/assets/123",
      browser_download_url:
        "https://github.com/OpenFuckJob/FuckJob/releases/download/v0.1.6/OfferFlow_0.1.6_x64-setup.exe",
    },
  ],
};

test("rewrites GitHub API asset URLs to public browser downloads", () => {
  const updater = {
    version: "0.1.6",
    platforms: {
      "windows-x86_64": {
        signature: "signature",
        url: "https://api.github.com/repos/OpenFuckJob/FuckJob/releases/assets/123",
      },
      "windows-x86_64-nsis": {
        signature: "signature",
        url: "https://api.github.com/repos/OpenFuckJob/FuckJob/releases/assets/123",
      },
    },
  };

  const result = rewriteUpdaterUrls(updater, release);

  assert.equal(
    result.platforms["windows-x86_64"].url,
    release.assets[0].browser_download_url,
  );
  assert.equal(
    result.platforms["windows-x86_64-nsis"].url,
    release.assets[0].browser_download_url,
  );
  assert.match(updater.platforms["windows-x86_64"].url, /api\.github\.com/);
  assert.equal(result.notes, release.body);
});

test("aliases darwin platform keys for compatibility with both tauri updater formats", () => {
  const macRelease = {
    ...release,
    assets: [
      {
        id: 456,
        name: "OfferFlow_0.1.6_aarch64.app.tar.gz",
        url: "https://api.github.com/repos/OpenFuckJob/FuckJob/releases/assets/456",
        browser_download_url:
          "https://github.com/OpenFuckJob/FuckJob/releases/download/v0.1.6/OfferFlow_0.1.6_aarch64.app.tar.gz",
      },
    ],
  };
  const updater = {
    version: "0.1.6",
    platforms: {
      "darwin-aarch64": {
        signature: "sig",
        url: "https://api.github.com/repos/OpenFuckJob/FuckJob/releases/assets/456",
      },
    },
  };
  const result = rewriteUpdaterUrls(updater, macRelease);
  assert.equal(
    result.platforms["darwin-aarch64"].url,
    macRelease.assets[0].browser_download_url,
  );
  assert.equal(
    result.platforms["darwin-aarch64-app"].url,
    macRelease.assets[0].browser_download_url,
  );
});

test("keeps updater notes when the release has no description", () => {
  const publicUrl = release.assets[0].browser_download_url;
  const result = rewriteUpdaterUrls(
    { notes: "原始说明", platforms: { "windows-x86_64": { url: publicUrl } } },
    { ...release, body: "  " },
  );

  assert.equal(result.notes, "原始说明");
});

test("rejects API URLs that reference a missing release asset", () => {
  assert.throws(
    () =>
      rewriteUpdaterUrls(
        {
          platforms: {
            "windows-x86_64": {
              url: "https://api.github.com/repos/OpenFuckJob/FuckJob/releases/assets/999",
            },
          },
        },
        release,
      ),
    /不存在的 Release 资产 999/,
  );
});

test("accepts an already-public URL for the same release", () => {
  const publicUrl = release.assets[0].browser_download_url;
  const result = rewriteUpdaterUrls(
    { platforms: { "windows-x86_64": { url: publicUrl } } },
    release,
  );

  assert.equal(result.platforms["windows-x86_64"].url, publicUrl);
});

test("rewrites draft untagged asset URLs to the final tagged release URL", () => {
  const draftRelease = {
    ...release,
    assets: [
      {
        ...release.assets[0],
        browser_download_url:
          "https://github.com/OpenFuckJob/FuckJob/releases/download/untagged-f484c740cbb349d3be94/OfferFlow_0.1.6_x64-setup.exe",
      },
    ],
  };
  const result = rewriteUpdaterUrls(
    {
      platforms: {
        "windows-x86_64": {
          url: draftRelease.assets[0].browser_download_url,
        },
      },
    },
    draftRelease,
  );

  assert.equal(
    result.platforms["windows-x86_64"].url,
    "https://github.com/OpenFuckJob/FuckJob/releases/download/v0.1.6/OfferFlow_0.1.6_x64-setup.exe",
  );
});
