#!/usr/bin/env node
// Deep-merges an Altius branding overlay over a base VS Code OSS
// product.json, writing the result to `out`. `overlay` values win on
// conflicts; arrays are replaced wholesale rather than concatenated.
"use strict";

const fs = require("fs");

const [, , basePath, overlayPath, outPath] = process.argv;
if (!basePath || !overlayPath || !outPath) {
  console.error(
    "usage: merge-product-json.js <base.json> <overlay.json> <out.json>",
  );
  process.exit(1);
}

function readJson(path) {
  return JSON.parse(fs.readFileSync(path, "utf8"));
}

function deepMerge(base, overlay) {
  if (Array.isArray(overlay)) return overlay;
  if (typeof overlay !== "object" || overlay === null) return overlay;
  const out = { ...(typeof base === "object" && base ? base : {}) };
  for (const [key, value] of Object.entries(overlay)) {
    out[key] = deepMerge(out[key], value);
  }
  return out;
}

const base = readJson(basePath);
const overlay = readJson(overlayPath);
const merged = deepMerge(base, overlay);

fs.writeFileSync(outPath, JSON.stringify(merged, null, 2) + "\n");
console.log(`wrote ${outPath}`);
