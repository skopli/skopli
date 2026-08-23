// @ts-check
import { defineConfig } from "astro/config";
import { fileURLToPath } from "node:url";
import { writeFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import starlight from "@astrojs/starlight";
import tokyoNight from "@shikijs/themes/tokyo-night";

// Restrain tokyo-night's syntax palette (L-5): its violet function/keyword hue
// (#bb9af7 / #9d7cd8) introduces a second accent that fights the cobalt --accent.
// Remap both purples to the shared --code-fn cobalt-family cyan so docs code
// matches the landing's restrained highlight palette. No other colours change.
/** @type {any} */
const tokyoNightRestrained = {
  ...tokyoNight,
  settings: (tokyoNight.settings ?? tokyoNight.tokenColors ?? []).map((/** @type {any} */ s) => {
    const fg = s?.settings?.foreground?.toLowerCase();
    if (fg === "#bb9af7" || fg === "#9d7cd8") {
      return { ...s, settings: { ...s.settings, foreground: "#89c4e6" } };
    }
    return s;
  }),
};

const ROOT_REDIRECT = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta http-equiv="refresh" content="0; url=/skopli/" />
    <link rel="canonical" href="https://docs.skopli.com/skopli/" />
    <title>Redirecting to Skopli docs</title>
  </head>
  <body>
    <p>Redirecting to <a href="/skopli/">/skopli/</a>&hellip;</p>
  </body>
</html>
`;

/**
 * The whole site (including Pagefind and the sitemap) builds natively into
 * dist/skopli/ via `outDir`, so the on-disk tree matches the /skopli/ base in
 * every generated link. This integration only drops a meta-refresh redirect at
 * the true domain root (dist/index.html): docs.skopli.com/ -> /skopli/.
 * Deploy dist/ at https://docs.skopli.com/ and everything lines up.
 * @returns {import('astro').AstroIntegration}
 */
function rootRedirect() {
  return {
    name: "skopli-root-redirect",
    hooks: {
      "astro:build:done": async ({ dir }) => {
        // dir is <project>/dist/skopli (the outDir); the domain root is its parent.
        const outPath = fileURLToPath(dir);
        const domainRoot = join(outPath, "..");
        await mkdir(domainRoot, { recursive: true });
        await writeFile(join(domainRoot, "index.html"), ROOT_REDIRECT, "utf8");
      },
    },
  };
}

export default defineConfig({
  site: "https://docs.skopli.com",
  base: "/skopli/",
  outDir: "./dist/skopli",
  integrations: [
    rootRedirect(),
    starlight({
      title: "Skopli",
      favicon: "/favicon.svg",
      logo: {
        light: "./src/assets/skopli-mark-light.svg",
        dark: "./src/assets/skopli-mark-dark.svg",
        replacesTitle: false,
      },
      customCss: ["./src/styles/tokens.css", "./src/styles/custom.css"],
      expressiveCode: {
        // Inline the EC styles instead of the external ec.*.css: the external
        // sheet is injected in the body with the first code block and arrives
        // after first paint, flashing unstyled code on every navigation.
        emitExternalStylesheet: false,
        // Dark first (Starlight uses themes[0] for dark, themes[1] for light).
        // tokyo-night approximates the shared palette: kw blue, str green,
        // num amber, cm grey. Its function/identifier violet is remapped to the
        // cobalt-family cyan below so no second accent competes with --accent
        // (matches the landing's restrained --code-fn). Light uses github-light.
        themes: [tokyoNightRestrained, "github-light"],
        styleOverrides: {
          // Frames sit on the brightest instrument plane (--code-0) with the
          // landing's hairline code-edge border and flat radius.
          borderColor: "var(--code-edge)",
          borderRadius: "var(--r-3)",
          borderWidth: "var(--hair)",
          codeBackground: "var(--code-0)",
          frames: {
            editorActiveTabBackground: "var(--ground-1)",
            editorTabBarBackground: "var(--ground-1)",
            editorBackground: "var(--code-0)",
            terminalBackground: "var(--code-0)",
            terminalTitlebarBackground: "var(--ground-1)",
            frameBoxShadowCssValue: "none",
          },
        },
      },
      description:
        "Read AI coding-agent usage from 40 harnesses offline, and price it against live market catalogs.",
      tagline:
        "Read AI coding-agent usage from 40 harnesses offline, and price it against live market catalogs. An SDK, not a CLI.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/skopli/skopli",
        },
      ],
      components: {
        Footer: "./src/components/Footer.astro",
        PageTitle: "./src/components/PageTitle.astro",
        ThemeProvider: "./src/components/ThemeProvider.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
      },
      sidebar: [
        {
          label: "Guide",
          items: [
            { label: "Getting started", slug: "guide/getting-started" },
            { label: "Reading usage", slug: "guide/reading" },
            { label: "Rollups", slug: "guide/rollups" },
            { label: "Pricing basics", slug: "guide/pricing-basics" },
          ],
        },
        {
          label: "Pricing deep dive",
          items: [
            { label: "Model matching", slug: "pricing/model-matching" },
            {
              label: "Cost math & cache-write splits",
              slug: "pricing/cost-math",
            },
            {
              label: "Long-context tiers",
              slug: "pricing/long-context-tiers",
            },
            {
              label: "Rollup pricing vs per-event pricing",
              slug: "pricing/rollups-vs-events",
            },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "Supported harnesses", slug: "reference/harnesses" },
            { label: "Coverage", slug: "reference/coverage" },
            { label: "API surface", slug: "reference/api" },
          ],
        },
      ],
    }),
  ],
});
