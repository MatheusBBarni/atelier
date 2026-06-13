import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import {
  buildLlmsFullTxt,
  fromCollectionEntry,
} from "../lib/machineSurfaces.mjs";

// Prerendered to a static `/llms-full.txt`: the concatenated raw Markdown of
// every non-`llms_optional` entry, each as `# {title}` + body.
export const prerender = true;

export const GET: APIRoute = async () => {
  const entries = (await getCollection("docs")).map(fromCollectionEntry);
  const body = buildLlmsFullTxt({ entries });
  return new Response(body, {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
};
