import { harnesses, label } from "./harnesses.ts";
import { langs } from "./langs.ts";
import { command, quickstarts, releaseNote } from "./quickstart.ts";

export const quickstartMd = [
  ...(releaseNote ? [releaseNote] : []),
  ...quickstarts.map(
    (q) =>
      `### ${langs[q.id]}\n\nInstall: \`${command(q)}\`${q.note ? `\n\n${q.note}` : ""}\n\n\`\`\`${q.id}\n${q.code}\n\`\`\``,
  ),
].join("\n\n");

export const harnessMd = [
  "| Harness | id |",
  "| --- | --- |",
  ...harnesses.map((h) => `| ${label(h)} | \`${h.id}\` |`),
].join("\n");
