import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";
import { harnesses, unsupported } from "@skopli/ui/data/harnesses.ts";

// The Markdown twin: the page source with its title as an H1, served at the canonical .md URL.
export const getStaticPaths: GetStaticPaths = async () =>
  (await getCollection("docs")).map((entry) => ({ params: { slug: entry.id }, props: { entry } }));

export const GET: APIRoute = ({ props }) => {
  const { entry } = props as { entry: Awaited<ReturnType<typeof getCollection<"docs">>>[number] };
  const extra = entry.id === "reference/harnesses" ? harnessTable() : "";
  const body = `# ${entry.data.title}\n\n${entry.data.description}\n\n${extra}${entry.body ?? ""}`;
  return new Response(body, { headers: { "content-type": "text/markdown; charset=utf-8" } });
};

function harnessTable(): string {
  const rows = harnesses.map(
    (h, i) =>
      `| ${i + 1} | ${h.name} | \`${h.id}\` | ${h.fields.join(", ") || "-"} | ${h.estimated ? "estimated" : "reported"} |`,
  );
  return `## The list\n\nIds are stable and lower-case; display names follow each product's own branding.\n\n| # | Harness | id | Fields in fixture | Counts |\n| --- | --- | --- | --- | --- |\n${rows.join("\n")}\n\nDetected but not read: ${unsupported.join(", ")}.\n\n`;
}
