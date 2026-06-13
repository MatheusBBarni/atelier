import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { buildTwin, fromCollectionEntry } from "../../lib/machineSurfaces.mjs";

// Prerendered raw-Markdown twin of each doc page at `/docs/<slug>.md`. Uses a
// rest param (mirroring `[...slug].astro`); one static path per collection
// entry. Generated reference ids are flattened to clean slugs (`configuration`,
// `cli`) by the collection's `generateId`, so twins land at `/docs/cli.md` etc.
export const prerender = true;

export async function getStaticPaths() {
  const docs = await getCollection("docs");
  return docs.map((entry) => ({
    params: { slug: entry.id },
    props: { entry: fromCollectionEntry(entry) },
  }));
}

export const GET: APIRoute = ({ props }) => {
  const body = buildTwin(props.entry);
  return new Response(body, {
    headers: { "Content-Type": "text/markdown; charset=utf-8" },
  });
};
