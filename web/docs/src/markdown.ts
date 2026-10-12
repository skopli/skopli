import { harnesses } from "@skopli/ui/harnesses";
import { langs } from "@skopli/ui/langs";
import { command, quickstarts, releaseNote } from "@skopli/ui/quickstart";

const quickstartMd = [
  ...(releaseNote ? [releaseNote] : []),
  ...quickstarts.map(
    (q) =>
      `### ${langs[q.id]}\n\nInstall: \`${command(q)}\`${q.note ? `\n\n${q.note}` : ""}\n\n\`\`\`${q.id}\n${q.code}\n\`\`\``,
  ),
].join("\n\n");

const harnessMd = [
  "| Harness | id |",
  "| --- | --- |",
  ...harnesses.map((h) => `| ${h.name} | \`${h.id}\` |`),
].join("\n");

export const toMarkdown = (body: string): string =>
  body
    .replace(/^import \w+ from "[^"]*\.astro";\n/gm, "")
    .replace(/^<Quickstart \/>$/m, quickstartMd)
    .replace(/^<HarnessTable \/>$/m, harnessMd)
    .replace(/^<\/?Tabs>\n?/gm, "")
    .replace(
      /^<Fragment slot="(\w+)">\n/gm,
      (_, lang: keyof typeof langs) => `### ${langs[lang]}\n\n`,
    )
    .replace(/^<\/Fragment>\n?/gm, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

export const pageMarkdown = (
  title: string,
  description: string,
  body: string | undefined,
): string => `# ${title}\n\n> ${description}\n\n${toMarkdown(body ?? "")}\n`;
