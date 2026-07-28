import assert from "node:assert/strict";
import test from "node:test";

import { themeCatalogDocument } from "./build-multi-theme-media-pack.mjs";

test("builds deterministic theme paths and keeps classic as a compatibility option", () => {
  const catalog = themeCatalogDocument({
    default_theme_id: "windows-tech",
    themes: [{
      id: "windows-tech",
      display_name_key: "deviceSimulator.mediaThemes.windowsTech",
    }],
  }, true);
  assert.equal(catalog.schema_version, 1);
  assert.equal(catalog.default_theme_id, "windows-tech");
  assert.deepEqual(catalog.themes.map((theme) => theme.id), ["classic", "windows-tech"]);
  assert.equal(
    catalog.themes[1].streams.third,
    "media/themes/windows-tech/third/media.json",
  );
});
