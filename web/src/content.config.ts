import { defineCollection, z } from "astro:content";
import { glob } from "astro/loaders";

// The single `docs` collection backing the whole `/docs` section. It mixes
// committed prose (Quickstart, Concepts, Governance — task_04–06) with
// git-ignored generated reference under `_generated/` (Configuration, CLI —
// task_11). The `**/*.md` pattern intentionally includes `_`-prefixed paths so
// the generated fragments are picked up; do not switch to `[^_]` excluding.
const docs = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./src/content/docs" }),
  schema: z.object({
    /** Page `<title>` and the section-nav / index label. */
    title: z.string(),
    /** Ordering key for the section nav and the `/docs` index. */
    nav_order: z.number(),
    /** When true, the page is bucketed under `## Optional` in `llms.txt` (task_07). */
    llms_optional: z.boolean().default(false),
  }),
});

export const collections = { docs };
