import type { Locale } from "@skopli/ui/i18n";
import { absoluteHref, t } from "./i18n.ts";
import { toMarkdown } from "./markdown.ts";
import { docsFor, markdownPage, mdPath, orderedDocs, splitId } from "./pages.ts";
import { nav } from "../nav.ts";

export async function llmsIndex(locale: Locale): Promise<string> {
  const docs = await docsFor(locale);
  const home = docs.find((d) => splitId(d.id).slug === "");
  const lines = [`# ${t(locale)("docs")}: Skopli`, "", `> ${home?.data.description ?? ""}`, ""];
  for (const group of nav) {
    const items = group.items.flatMap((item) => {
      const doc = docs.find((d) => splitId(d.id).slug === item.slug);
      return doc
        ? [
            `- [${doc.data.title}](${absoluteHref(mdPath(item.slug), locale)}): ${doc.data.description}`,
          ]
        : [];
    });
    if (items.length) lines.push(`## ${group.label}`, "", ...items, "");
  }
  return `${lines.join("\n").trim()}\n`;
}

export async function llmsFull(locale: Locale): Promise<string> {
  const docs = orderedDocs(await docsFor(locale));
  return docs.map((doc) => toMarkdown(markdownPage(doc))).join("\n---\n\n");
}
