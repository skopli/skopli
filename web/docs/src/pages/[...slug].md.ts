import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection, type CollectionEntry } from "astro:content";
import { HARNESSES, HARNESS_NOTES } from "@skopli/ui/data/harnesses";
import { LANGUAGES } from "@skopli/ui/data/languages";
import { localeFromPath, useTranslations, type Locale } from "@skopli/ui/i18n";

const harnessTable = (locale: Locale): string => {
  const t = useTranslations(locale);
  const rows = HARNESSES.map(
    (h, i) => `| ${i + 1} | ${h.name}${h.note ? ` [^${h.note}]` : ""} | \`${h.id}\` |`,
  );
  const notes = HARNESS_NOTES.map((n) => `[^${n}]: ${t(`harness.note.${n}`)}`);
  const head = `| ${t("harness.col.index")} | ${t("harness.col.name")} | ${t("harness.col.id")} |`;
  return [head, "| --: | --- | --- |", ...rows, "", ...notes].join("\n");
};

const installTabs = (locale: Locale): string => {
  const t = useTranslations(locale);
  const fence = (lang: string, code: string) => `\`\`\`${lang}\n${code}\n\`\`\``;
  return LANGUAGES.map((l) =>
    [
      `**${l.name}**`,
      l.note ? t(`install.note.${l.note}`) : "",
      ...("options" in l
        ? l.options.map((s) => `**${s.label}**\n\n${fence(s.lang, s.code)}`)
        : l.install.map((s) => fence(s.lang, s.code))),
    ]
      .filter(Boolean)
      .join("\n\n"),
  ).join("\n\n");
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
    .replace(/^import .* from "(?:@skopli\/ui|\.{1,2})\/.*;\n?/gm, "")
    .replace(/<HarnessTable\b[^>]*\/>/g, harnessTable(localeFromPath(`/${entry.id}`, "/")))
    .replace(/<InstallTabs\b[^>]*\/>/g, installTabs(localeFromPath(`/${entry.id}`, "/")))
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
