import { existsSync } from "node:fs";
import { captureScreens } from "@skopli/ui/scripts/screens.ts";
import { navItems } from "../src/nav.ts";

const only = process.argv.slice(2);
const dist = new URL("../dist", import.meta.url).pathname;
const pages = [
  { name: "index", path: "/skopli/" },
  ...navItems.map((item) => ({ name: item.slug.replace("/", "-"), path: `/skopli/${item.slug}` })),
  { name: "404", path: "/skopli/this-page-does-not-exist" },
  ...(existsSync(`${dist}/skopli/en-XA.html`)
    ? [
        { name: "en-XA-index", path: "/skopli/en-XA" },
        { name: "en-XA-guide-getting-started", path: "/skopli/en-XA/guide/getting-started" },
      ]
    : []),
].filter((page) => !only.length || only.includes(page.name));

await captureScreens({
  pages,
  dist,
  notFound: "skopli/404.html",
  outDir: new URL("../../../artifacts/screens/docs/", import.meta.url).pathname,
  port: 4399,
});
