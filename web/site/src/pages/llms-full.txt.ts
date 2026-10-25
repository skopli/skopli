import type { APIRoute } from "astro";
import { harnessMd, quickstartMd } from "@skopli/ui/markdown";
import { project } from "@skopli/ui/project";
import { description, layers, rules, title } from "../content.ts";

const plain = (html: string): string => html.replace(/<\/?code>/g, "`");

const body = [
  `# ${title}`,
  "",
  `> ${description}`,
  "",
  "## Three layers, used separately or together",
  "",
  ...layers.map((l) => `- **${l.name}** (network: ${l.network}): ${plain(l.does)}`),
  "",
  "## Cost math",
  "",
  ...rules.map(([term, rule]) => `- ${term}: ${plain(rule)}`),
  "",
  "## Harnesses",
  "",
  harnessMd,
  "",
  "## Quickstart in each language",
  "",
  quickstartMd,
  "",
  "## Links",
  "",
  `- [Documentation](${project.docs})`,
  `- [Documentation index for language models](${project.docs}llms.txt)`,
  `- [Full documentation in one file](${project.docs}llms-full.txt)`,
  `- [Source](${project.github})`,
  "",
].join("\n");

export const GET: APIRoute = () =>
  new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
