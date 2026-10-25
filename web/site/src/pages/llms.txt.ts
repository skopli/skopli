import type { APIRoute } from "astro";
import { project } from "@skopli/ui/project";
import { description } from "../content.ts";

const { docs, github, site } = project;
const body = [
  `# ${project.name}`,
  "",
  `> ${description}`,
  "",
  "Reading and rollups run offline against local files. Pricing is a separate step that fetches market catalogs by default, caches them for an hour, keeps pricing from that cache when the network is gone, and leaves unknown models as `priced: false` instead of guessing.",
  "",
  "## Links",
  "",
  `- [This page as Markdown](${site}llms-full.txt): the marketing site in one file.`,
  `- [Documentation](${docs}): guides and reference. Every page has a Markdown twin (swap the trailing \`/\` for \`.md\`).`,
  `- [llms.txt](${docs}llms.txt): index of the documentation pages.`,
  `- [llms-full.txt](${docs}llms-full.txt): the whole documentation in one file.`,
  `- [Getting started](${docs}guide/getting-started/): install for each language and the first priced rollup.`,
  `- [GitHub](${github}): source, issues, releases.`,
  "",
].join("\n");

export const GET: APIRoute = () =>
  new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
