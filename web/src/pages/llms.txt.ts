import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import {
  buildLlmsTxt,
  fromCollectionEntry,
} from "../lib/machineSurfaces.mjs";

// Prerendered to a static `/llms.txt`. The curated LLM index: an H1, a
// blockquote summary, a `## Docs` link section, and a `## Optional` bucket
// driven by each entry's `llms_optional` frontmatter flag.
export const prerender = true;

export const GET: APIRoute = async ({ site }) => {
  const entries = (await getCollection("docs")).map(fromCollectionEntry);
  const body = buildLlmsTxt({
    entries,
    siteOrigin: site?.origin ?? "",
    rawBase: import.meta.env.BASE_URL,
  });
  return new Response(body, {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
};
