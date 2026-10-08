import { existsSync } from "node:fs";
import { captureScreens } from "@skopli/ui/scripts/screens.ts";

const only = process.argv.slice(2);
const dist = new URL("../dist", import.meta.url).pathname;
const pages = [
  { name: "index", path: "/" },
  { name: "404", path: "/this-page-does-not-exist" },
  ...(existsSync(`${dist}/en-XA/index.html`) ? [{ name: "en-XA-index", path: "/en-XA" }] : []),
].filter((page) => !only.length || only.includes(page.name));

await captureScreens({
  pages,
  dist,
  notFound: "404.html",
  outDir: new URL("../../../artifacts/screens/site/", import.meta.url).pathname,
  port: 4398,
});
