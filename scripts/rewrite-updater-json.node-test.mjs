import assert from "node:assert/strict";
import test from "node:test";
import { rewriteUpdaterUrls } from "./rewrite-updater-json.mjs";

const release = {
  tag_name: "v0.1.6",
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
