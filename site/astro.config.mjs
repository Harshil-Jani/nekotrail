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
  // The Conductor blog post repeats the guide and points its canonical at it, so only the guide is listed.
  // No lastmod: a build date on every URL is worse than none.
  integrations: [
    sitemap({ filter: (page) => !page.includes("/404") && !page.endsWith("/blog/moving-from-conductor/") }),
  ],
  devToolbar: { enabled: false },
});
