// Pure helpers for the build-time docs generation step (`npm run generate`).
// Kept separate from the I/O-driving `generate-docs.mjs` so the composition
// logic is unit-testable under `node --test` without spawning the Rust binary.

/**
 * Append the hand-written `[presets.*]` reference onto the generated
 * Configuration page.
 *
 * `[presets.*]` is absent from the merged config emitted by `atelier
 * --emit-docs` (presets are only materialized once one is selected), so the
 * reference is hand-written (see `presets-reference.md`) and stitched onto the
 * SAME Configuration page here — the generated page already announces that
 * presets are "documented separately". Because the generator rewrites
 * `configuration.md` from scratch on every run, this append is idempotent: it
 * always starts from the freshly generated body.
 *
 * @param {string} configMarkdown - the generated `configuration.md` contents.
 * @param {string} presetsMarkdown - the hand-written presets section.
 * @returns {string} the combined Markdown for the Configuration page.
 */
export function appendPresetsSection(configMarkdown, presetsMarkdown) {
  const base = configMarkdown.replace(/\s*$/, "");
  const presets = presetsMarkdown.trim();
  return `${base}\n\n${presets}\n`;
}
