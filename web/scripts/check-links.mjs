// Local equivalent of the `web-checks` CI link gate (.github/workflows/web-checks.yml).
// Stages `dist/` under an `atelier/` directory so root-relative links
// (/atelier/docs/...) resolve the way GitHub Pages serves them, then runs
// lychee in --offline mode over the HTML. A link missing the base (/docs/...)
// — a production 404 — fails; external links are skipped to avoid flakiness.
//
// Usage (lychee must be installed: `brew install lychee` / `cargo install lychee`):
//   GITHUB_PAGES=true npm run build && npm run check:links
import { cpSync, mkdirSync, rmSync, existsSync, readdirSync, statSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const cwd = process.cwd();
const dist = join(cwd, "dist");
const site = join(cwd, "_site");
const staged = join(site, "atelier");

if (!existsSync(dist)) {
  console.error("✖ dist/ not found — run `GITHUB_PAGES=true npm run build` first.");
  process.exit(1);
}

// Stage dist under the /atelier base path.
rmSync(site, { recursive: true, force: true });
mkdirSync(staged, { recursive: true });
cpSync(dist, staged, { recursive: true });

// Drop the `.md` LLM twins: their relative links target the rendered HTML
// routes, not sibling files. They are validated separately by `check:surfaces`.
const dropMarkdown = (dir) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) dropMarkdown(p);
    else if (name.endsWith(".md")) unlinkSync(p);
  }
};
dropMarkdown(staged);

const lychee = spawnSync(
  "lychee",
  ["--offline", "--root-dir", site, staged],
  { stdio: "inherit" },
);

if (lychee.error) {
  console.error(
    "✖ could not run lychee — install it with `brew install lychee` or `cargo install lychee`.",
  );
  process.exit(1);
}
process.exit(lychee.status ?? 1);
