// Integration check for the prerendered machine-readable surfaces. Run after
// `astro build` (ideally with GITHUB_PAGES=true). Asserts the four surfaces are
// emitted as static files and that every URL is absolute and base-correct.
// Used by the local build and the `web-checks` CI gate (task_08).
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

const dist = join(process.cwd(), "dist");
const expectBase = process.env.GITHUB_PAGES === "true" ? "/atelier/" : "/";
const origin = "https://matheusbbarni.github.io";

const errors = [];
const ok = (cond, msg) => {
  if (!cond) errors.push(msg);
};

const read = (rel) => {
  const p = join(dist, rel);
  if (!existsSync(p)) {
    errors.push(`missing prerendered file: ${rel}`);
    return "";
  }
  return readFileSync(p, "utf8");
};

// 1. All four surfaces prerendered.
const llms = read("llms.txt");
const llmsFull = read("llms-full.txt");
const sitemap = read("sitemap.xml");

// At least one per-page `.md` twin exists.
const twins = existsSync(join(dist, "docs"))
  ? readdirSync(join(dist, "docs")).filter((f) => f.endsWith(".md"))
  : [];
ok(twins.length > 0, "no per-page .md twins emitted under dist/docs");

// 2. llms.txt structure: one H1, a blockquote, sections, an Optional bucket.
const h1Count = llms.split("\n").filter((l) => /^# /.test(l)).length;
ok(h1Count === 1, `llms.txt should have exactly one H1, found ${h1Count}`);
ok(/\n> /.test(`\n${llms}`), "llms.txt missing blockquote summary");
ok(llms.includes("## Optional"), "llms.txt missing ## Optional section");

// 3. Every URL in llms.txt + sitemap is absolute and base-correct.
const wanted = `${origin}${expectBase}`;
const urls = [
  ...llms.matchAll(/\((https?:\/\/[^)]+)\)/g),
  ...sitemap.matchAll(/<loc>(https?:\/\/[^<]+)<\/loc>/g),
].map((m) => m[1]);
ok(urls.length > 0, "no URLs found in llms.txt or sitemap");
for (const u of urls) {
  ok(u.startsWith(wanted), `URL not absolute/base-correct (want ${wanted}): ${u}`);
}

// 4. llms-full.txt carries page bodies (non-empty, markdown headings present).
ok(/^#\s/m.test(llmsFull), "llms-full.txt has no page content");

if (errors.length > 0) {
  console.error("✖ machine-surface checks failed:");
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
console.log(
  `✓ machine surfaces OK (base ${expectBase}, ${twins.length} twin(s), ${urls.length} URL(s))`,
);
