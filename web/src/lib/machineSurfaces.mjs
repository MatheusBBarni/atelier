// Pure builders for the machine-readable surfaces (`llms.txt`, `llms-full.txt`,
// per-page `.md` twins, and the sitemap). Kept as plain JS (not `.ts`) so the
// same logic is importable by both the Astro `.ts` endpoints and the
// `node --test` unit suite without a TypeScript loader. The endpoints stay thin:
// they read `getCollection('docs')`, normalize each entry to the shape below,
// and call these functions.
//
// An entry is `{ id, title, nav_order, llms_optional, body }` where `body` is the
// raw Markdown WITHOUT frontmatter (Astro strips it). Because the title lives in
// frontmatter, the twin and `llms-full.txt` prepend `# {title}` so each emitted
// document is a complete, citable Markdown file.

/** One-line summary used in the `llms.txt` blockquote. */
export const SITE_TITLE = "Atelier";
export const SITE_SUMMARY =
  "Atelier is a terminal-native harness that routes each prompt through an " +
  "orchestrator and a sequence of specialized agents, backed by pluggable " +
  "agent-CLI runtimes. This is its documentation: quickstart, concepts, " +
  "governance, configuration, and CLI reference.";

/** Normalize a base path to a leading-and-trailing-slash form (e.g. `/atelier/`). */
export function normalizeBase(rawBase) {
  const withLead = rawBase.startsWith("/") ? rawBase : `/${rawBase}`;
  return withLead.endsWith("/") ? withLead : `${withLead}/`;
}

/**
 * Build an absolute, base-correct URL for a path under the site.
 * `siteOrigin` is e.g. `https://matheusbbarni.github.io`; `rawBase` is
 * `import.meta.env.BASE_URL` (`/atelier/` under GitHub Pages, else `/`).
 */
export function absoluteUrl(siteOrigin, rawBase, path) {
  const base = normalizeBase(rawBase);
  const rel = path.replace(/^\//, "");
  return `${siteOrigin.replace(/\/$/, "")}${base}${rel}`;
}

/** Absolute URL of a doc page's rendered HTML route (trailing slash). */
export function docPageUrl(siteOrigin, rawBase, id) {
  return absoluteUrl(siteOrigin, rawBase, `docs/${id}/`);
}

/** Flatten an Astro `getCollection('docs')` entry into the shape used here. */
export function fromCollectionEntry(entry) {
  return {
    id: entry.id,
    title: entry.data.title,
    nav_order: entry.data.nav_order,
    llms_optional: entry.data.llms_optional ?? false,
    body: entry.body ?? "",
  };
}

/** Entries sorted by `nav_order`, then `id` for stable ordering. */
export function sortEntries(entries) {
  return [...entries].sort(
    (a, b) => a.nav_order - b.nav_order || a.id.localeCompare(b.id),
  );
}

/**
 * Extract a short note for the `llms.txt` link list: the first non-empty,
 * non-heading paragraph of the body, collapsed to one line and truncated.
 */
export function firstParagraph(body, maxLen = 160) {
  const lines = body.split(/\r?\n/);
  const collected = [];
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed === "") {
      if (collected.length > 0) break;
      continue;
    }
    if (trimmed.startsWith("#")) {
      if (collected.length > 0) break;
      continue;
    }
    collected.push(trimmed);
  }
  let note = collected.join(" ").replace(/\s+/g, " ").trim();
  if (note.length > maxLen) {
    const slice = note.slice(0, maxLen);
    const lastSpace = slice.lastIndexOf(" ");
    note = `${(lastSpace > 0 ? slice.slice(0, lastSpace) : slice).trim()}…`;
  }
  return note;
}

/** A complete Markdown document for an entry: `# {title}` + raw body. */
export function buildTwin(entry) {
  const body = entry.body.trim();
  return `# ${entry.title}\n\n${body}\n`;
}

/**
 * `llms.txt`: an H1, a blockquote summary, a `## Docs` section of
 * `[name](url): note` links for non-optional entries, and a `## Optional`
 * bucket for entries flagged `llms_optional`.
 */
export function buildLlmsTxt({ entries, siteOrigin, rawBase }) {
  const sorted = sortEntries(entries);
  const link = (e) =>
    `- [${e.title}](${docPageUrl(siteOrigin, rawBase, e.id)}): ${firstParagraph(e.body)}`;

  const main = sorted.filter((e) => !e.llms_optional);
  const optional = sorted.filter((e) => e.llms_optional);

  const out = [`# ${SITE_TITLE}`, "", `> ${SITE_SUMMARY}`, ""];

  out.push("## Docs", "");
  for (const e of main) out.push(link(e));
  out.push("");

  out.push("## Optional", "");
  for (const e of optional) out.push(link(e));
  out.push("");

  return `${out.join("\n").trimEnd()}\n`;
}

/**
 * `llms-full.txt`: the concatenated raw Markdown of all non-`llms_optional`
 * entries (each as `# {title}` + body), separated by horizontal rules.
 */
export function buildLlmsFullTxt({ entries }) {
  const docs = sortEntries(entries).filter((e) => !e.llms_optional);
  return `${docs.map(buildTwin).join("\n---\n\n").trimEnd()}\n`;
}

/** `sitemap.xml`: the docs index plus every doc page route, absolute URLs. */
export function buildSitemap({ entries, siteOrigin, rawBase }) {
  const urls = [
    absoluteUrl(siteOrigin, rawBase, "docs/"),
    ...sortEntries(entries).map((e) => docPageUrl(siteOrigin, rawBase, e.id)),
  ];
  const body = urls
    .map((loc) => `  <url>\n    <loc>${loc}</loc>\n  </url>`)
    .join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${body}\n</urlset>\n`;
}
