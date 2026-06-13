// Unit tests for the machine-readable surface builders. Run with `node --test`.
// These exercise the pure logic in `src/lib/machineSurfaces.mjs` directly, so
// they cover `llms_optional` bucketing/exclusion and base-correct absolute URLs
// without depending on which prose pages happen to exist in the collection.
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  buildLlmsTxt,
  buildLlmsFullTxt,
  buildTwin,
  buildSitemap,
  absoluteUrl,
  docPageUrl,
  firstParagraph,
} from "../src/lib/machineSurfaces.mjs";

const SITE = "https://matheusbbarni.github.io";
const BASE = "/atelier/";

// A small fixture standing in for the collection: one optional page plus
// regular pages (mirrors the real Governance / Quickstart / Concepts shape).
const ENTRIES = [
  {
    id: "quickstart",
    title: "Quickstart",
    nav_order: 1,
    llms_optional: false,
    body: "Atelier routes every prompt through an orchestrator.\n\n## Install\n...",
  },
  {
    id: "governance",
    title: "Governance & Safety",
    nav_order: 3,
    llms_optional: false,
    body: "GOVERNANCE_BODY_MARKER approvals keep you in control.",
  },
  {
    id: "changelog",
    title: "Changelog",
    nav_order: 9,
    llms_optional: true,
    body: "OPTIONAL_BODY_MARKER release notes.",
  },
];

test("llms.txt has a single H1, a blockquote summary, link sections, and Optional", () => {
  const out = buildLlmsTxt({ entries: ENTRIES, siteOrigin: SITE, rawBase: BASE });
  const lines = out.split("\n");

  const h1s = lines.filter((l) => /^# /.test(l));
  assert.equal(h1s.length, 1, "exactly one H1");
  assert.equal(h1s[0], "# Atelier");

  assert.ok(
    lines.some((l) => l.startsWith("> ")),
    "has a blockquote summary",
  );
  assert.ok(out.includes("## Docs"), "has a Docs section");
  assert.ok(out.includes("## Optional"), "has an Optional section");

  // Non-optional entries appear as `[name](url): note` links under Docs.
  assert.ok(
    out.includes(`- [Quickstart](${docPageUrl(SITE, BASE, "quickstart")}): `),
    "Quickstart is a link with a note",
  );
});

test("llms.txt buckets llms_optional entries under ## Optional only", () => {
  const out = buildLlmsTxt({ entries: ENTRIES, siteOrigin: SITE, rawBase: BASE });
  const optionalIdx = out.indexOf("## Optional");
  const changelogIdx = out.indexOf("[Changelog]");
  const govIdx = out.indexOf("[Governance & Safety]");

  assert.ok(changelogIdx > optionalIdx, "optional entry sits below ## Optional");
  assert.ok(
    govIdx > -1 && govIdx < optionalIdx,
    "non-optional entry sits above ## Optional",
  );
});

test("llms-full.txt includes non-optional bodies and excludes llms_optional", () => {
  const out = buildLlmsFullTxt({ entries: ENTRIES });
  assert.ok(out.includes("# Governance & Safety"), "includes governance title");
  assert.ok(out.includes("GOVERNANCE_BODY_MARKER"), "includes governance body");
  assert.ok(
    !out.includes("OPTIONAL_BODY_MARKER"),
    "excludes the llms_optional entry body",
  );
});

test("twin returns the page title + raw Markdown body", () => {
  const twin = buildTwin(ENTRIES[1]);
  assert.ok(twin.startsWith("# Governance & Safety\n\n"), "leads with H1 title");
  assert.ok(twin.includes("GOVERNANCE_BODY_MARKER"), "contains the raw body");
});

test("sitemap lists the docs index and every doc page as absolute URLs", () => {
  const xml = buildSitemap({ entries: ENTRIES, siteOrigin: SITE, rawBase: BASE });
  assert.ok(xml.startsWith('<?xml version="1.0"'), "is XML");
  assert.ok(xml.includes(absoluteUrl(SITE, BASE, "docs/")), "has docs index");
  for (const e of ENTRIES) {
    assert.ok(
      xml.includes(`<loc>${docPageUrl(SITE, BASE, e.id)}</loc>`),
      `has ${e.id} page`,
    );
  }
});

test("every emitted URL is absolute and carries the /atelier base", () => {
  const llms = buildLlmsTxt({ entries: ENTRIES, siteOrigin: SITE, rawBase: BASE });
  const sitemap = buildSitemap({ entries: ENTRIES, siteOrigin: SITE, rawBase: BASE });
  // Links in llms.txt look like `(url):`; sitemap URLs are wrapped in <loc>.
  const llmsUrls = [...llms.matchAll(/\((https?:\/\/[^)]+)\)/g)].map((m) => m[1]);
  const locUrls = [...sitemap.matchAll(/<loc>(https?:\/\/[^<]+)<\/loc>/g)].map((m) => m[1]);
  const urls = [...llmsUrls, ...locUrls];
  assert.ok(urls.length > 0, "found URLs to check");
  for (const u of urls) {
    assert.ok(u.startsWith(`${SITE}/atelier/`), `absolute + based: ${u}`);
  }
});

test("base normalization: bare base still yields absolute URLs", () => {
  assert.equal(absoluteUrl(SITE, "/", "docs/x/"), `${SITE}/docs/x/`);
  assert.equal(docPageUrl(SITE, "/atelier", "x"), `${SITE}/atelier/docs/x/`);
});

test("firstParagraph skips headings and truncates", () => {
  assert.equal(firstParagraph("# Title\n\nHello world."), "Hello world.");
  const long = "word ".repeat(60);
  assert.ok(firstParagraph(long).endsWith("…"));
});
