import { defineConfig } from "astro/config";

const isGitHubPages = process.env.GITHUB_PAGES === "true";

export default defineConfig({
  site: "https://matheusbbarni.github.io",
  base: isGitHubPages ? "/atelier" : "/",
});
