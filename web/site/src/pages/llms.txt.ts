import type { APIRoute } from "astro";
import { harnesses } from "@skopli/ui/data/harnesses.ts";
import { en } from "../copy.ts";
import { docsUrl, releasesUrl, repoUrl } from "../site.ts";

const body = `# Skopli

> ${en.description}

Skopli is an SDK with no accounts and no telemetry.
Read and Rollup never touch the network.
Price fetches market catalogs (OpenRouter, LiteLLM, models.dev, or your own) by default when used, caches them, and reads from the cache offline; pass an empty sources list or offline for network-free pricing.
An unmatched model returns priced: false with the model keys it tried and stays out of every total.
Languages: TypeScript (Node 24 or newer, ESM only), Python, Rust, Ruby, Java, C#, Go, Swift, and C, all on one Rust core. MIT licensed.

## Links

- [Documentation](${docsUrl}): guides, pricing semantics, and the API reference, each page also available as Markdown.
- [Docs index for language models](${docsUrl}llms.txt)
- [Source and issues](${repoUrl})
- [Releases](${releasesUrl})

## Supported harnesses (${harnesses.length})

${harnesses.map((h) => `- ${h.name} (${h.id})`).join("\n")}
`;

export const GET: APIRoute = () =>
  new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
