import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import {
  buildSitemap,
  fromCollectionEntry,
} from "../lib/machineSurfaces.mjs";

// Hand-rolled sitemap (no integration / new dependency). Prerendered to a
// static `/sitemap.xml` listing the docs index and every doc page route as
// absolute, base-correct URLs.
export const prerender = true;

export const GET: APIRoute = async ({ site }) => {
  const entries = (await getCollection("docs")).map(fromCollectionEntry);
  const body = buildSitemap({
    entries,
    siteOrigin: site?.origin ?? "",
    rawBase: import.meta.env.BASE_URL,
  });
  return new Response(body, {
    headers: { "Content-Type": "application/xml; charset=utf-8" },
  });
};
