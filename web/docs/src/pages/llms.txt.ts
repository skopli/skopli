import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { mdPathFor, order } from "../nav.ts";

export const GET: APIRoute = async ({ site }) => {
  const entries = await getCollection("docs");
  const lines = order.map((id) => {
    const e = entries.find((x) => x.id === id);
    return e ? `- [${e.data.title}](${new URL(mdPathFor(id), site)}): ${e.data.description}` : "";
  });
  const text = `# Skopli

> Open-source SDK (MIT) that reads the session logs AI coding agents already keep on local disk, normalizes them into usage events, aggregates them, and prices the tokens against market catalogs. One Rust core, facades in TypeScript, Python, Ruby, Go, Java, C#, Swift and Rust, plus a C ABI. 40 harnesses. Read and Rollup never use the network; Price fetches catalogs by default and can run from its cache or with no sources.

Every page below is also served as Markdown at the linked .md URL.

## Docs

${lines.filter(Boolean).join("\n")}
`;
  return new Response(text, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
