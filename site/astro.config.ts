import { defineConfig } from "astro/config";
import tailwindcss from "@tailwindcss/vite";
import nimbus, {
  defineConfig as defineNimbusConfig,
} from "@cloudflare/nimbus-docs";
import { math, tableScroll } from "@cloudflare/nimbus-docs/markdown";
// From the Aiken VS Code extension (aiken-lang/vscode-aiken, Apache-2.0).
import aiken from "./src/lib/aiken.tmLanguage.json";

const nimbusConfig = defineNimbusConfig({
  // Canonical origin (no trailing slash). Drives canonical URLs, absolute OG
  // image URLs, robots.txt, sitemap, and the links in /llms.txt.
  site: "https://aiken-lang.com",
  title: "Aiken",
  description: "Cardano smart contract language and toolchain",
  locale: "en",
  github: "https://github.com/Midgard-Labs-Inc/aiken",
  socialImageAlt: "Aiken documentation preview",
  head: [{ tag: "meta", attrs: { name: "twitter:site", content: "@aiken_dev" } }],
});

export default defineConfig({
  // nimbus:adapter
  output: "static",
  // Tailwind v4 via its Vite plugin (the integration Astro recommends for
  // Tailwind v4 — replaces the PostCSS plugin, which doesn't build under
  // Astro 7's Vite 8 bundler).
  vite: {
    plugins: [tailwindcss()],
  },
  markdown: {
    // Shiki doesn't ship an Aiken grammar. Code fences use the lowercase name.
    shikiConfig: { langs: [{ ...aiken, name: "aiken" }] },
  },
  // Hover-prefetch link targets so full-page navigations feel instant without
  // a client-side router.
  prefetch: {
    prefetchAll: true,
    defaultStrategy: "hover",
  },
  integrations: [
    nimbus(nimbusConfig, {
      // Authoring rules are opt-in by design — your repo, your taste. The
      // two below are the load-bearing pair: frontmatter has to validate
      // against the content schema for the page to render properly, and
      // broken internal links are 404s for your readers. Add the others
      // (heading hierarchy, code-block language, style, etc.) when you're
      // ready to enforce them — see `nimbus-docs lint --help`.
      rules: {
        "nimbus/frontmatter-shape": "error",
        "nimbus/internal-link": "error",
      },
      markdown: {
        // Wrap wide tables so they scroll instead of overflowing the page
        // (styled by `.nb-table-scroll` in src/styles/prose.css).
        hastPlugins: [tableScroll(), math()],
        // `## Install {#install}` gives a heading a fixed id. A heading that
        // ends in other `{…}` text, such as `{account_id}` in a .md file, is
        // read as attributes too; remove this to keep it as text.
        features: { headingAttributes: true, math: true },
      },
    }),
  ],
});
