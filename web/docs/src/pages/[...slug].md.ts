import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection, type CollectionEntry } from "astro:content";
import { HARNESSES } from "@skopli/ui/data/harnesses";
import { useTranslations } from "@skopli/ui/i18n";

const NOTES = ["sessions", "estimated", "perModel"] as const;

const harnessTable = (): string => {
  const t = useTranslations("en");
  const rows = HARNESSES.map(
    (h, i) => `| ${i + 1} | ${h.name}${h.note ? ` [^${h.note}]` : ""} | \`${h.id}\` |`,
  );
  const notes = NOTES.map((n) => `[^${n}]: ${t(`harness.note.${n}`)}`);
  return ["| # | Harness | id |", "| --: | --- | --- |", ...rows, "", ...notes].join("\n");
};

const ledger = (block: string): string =>
  [...block.matchAll(/<dt>(.*?)<\/dt>\s*<dd[^>]*>(.*?)<\/dd>/gs)]
    .map(([, k, v]) => `- ${k.trim()}: ${v.trim()}`)
    .join("\n");

const flattenTabs = (body: string): string => {
  let depth = 0;
  return body
    .split("\n")
    .flatMap((line) => {
      const open = line.match(/^\s*<(Tabs|TabItem)\b([^>]*)>\s*$/);
      if (open) {
        depth++;
        const label = open[2]?.match(/label="([^"]*)"/)?.[1];
        return label ? [`**${label}**`, ""] : [];
      }
      if (/^\s*<\/(Tabs|TabItem)>\s*$/.test(line)) {
        depth--;
        return [];
      }
      return [line.replace(new RegExp(`^ {0,${depth * 2}}`), "")];
    })
    .join("\n");
};

export const toMarkdown = (entry: CollectionEntry<"docs">): string => {
  const body = flattenTabs(entry.body ?? "")
    .replace(/^import .*$\n?/gm, "")
    .replace(/<HarnessTable\b[^>]*\/>/g, harnessTable())
    .replace(/<dl class="ledger">[\s\S]*?<\/dl>/g, ledger)
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return `# ${entry.data.title}\n\n> ${entry.data.description}\n\n${body}\n`;
};

export const getStaticPaths: GetStaticPaths = async () =>
  (await getCollection("docs")).map((entry) => ({ params: { slug: entry.id }, props: { entry } }));

export const GET: APIRoute<{ entry: CollectionEntry<"docs"> }> = ({ props }) =>
  new Response(toMarkdown(props.entry), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
