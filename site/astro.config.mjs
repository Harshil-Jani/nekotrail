// The public site, built as static files for GitHub Pages. It is a project site, so every page lives
// under /nekotrail/ and every internal link goes through `href()` in src/lib/site.ts.
import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://harshil-jani.github.io",
  base: "/nekotrail/",
  trailingSlash: "always",
  output: "static",
  build: { format: "directory" },
  integrations: [sitemap({ filter: (page) => !page.includes("/404") })],
  devToolbar: { enabled: false },
});
