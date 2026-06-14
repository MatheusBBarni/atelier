// Shared base-path helpers so every page builds URLs that stay correct under
// the GitHub Pages `/atelier` base. Mirrors the original inline helper from
// `index.astro` so the landing page and all docs pages resolve assets and
// links the same way.
const rawBase = import.meta.env.BASE_URL;

export const basePath = rawBase.endsWith("/") ? rawBase : `${rawBase}/`;

export const assetPath = (path: string): string =>
  `${basePath}${path.replace(/^\//, "")}`;
