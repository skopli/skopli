import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { project } from "@skopli/ui/project";
import { SITE, mdPath, ordered } from "../nav";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const byId = new Map(docs.map((d) => [d.id, d]));
  const lines = ordered.flatMap(({ slug }) => {
    const entry = byId.get(slug);
    return entry
      ? [`- [${entry.data.title}](${SITE}${mdPath(slug)}): ${entry.data.description}`]
      : [];
  });
  const body = [
    `# ${project.name}`,
    "",
    `> ${project.description}`,
    "",
    "## Docs",
    "",
    ...lines,
    "",
    `- [Full docs in one file](${project.docs}llms-full.txt)`,
    "",
  ].join("\n");
  return new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
