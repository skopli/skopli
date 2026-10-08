import { captureScreens } from "@skopli/ui/scripts/screens.ts";

const only = process.argv.slice(2);
const pages = [
  { name: "index", path: "/" },
  { name: "404", path: "/this-page-does-not-exist" },
  ...(process.env.SKOPLI_PSEUDO_LOCALE === "1" ? [{ name: "en-XA-index", path: "/en-XA" }] : []),
].filter((page) => !only.length || only.includes(page.name));

await captureScreens({
  pages,
  dist: new URL("../dist", import.meta.url).pathname,
  notFound: "404.html",
  outDir: new URL("../../../artifacts/screens/site/", import.meta.url).pathname,
  port: 4398,
});
