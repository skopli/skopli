import { langs } from "@skopli/ui/langs";
import { harnessMd, quickstartMd } from "@skopli/ui/markdown";

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
