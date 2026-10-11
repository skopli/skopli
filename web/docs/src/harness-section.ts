import { harnesses, unsupported } from "@skopli/ui/data/harnesses.ts";

export const harnessHeading = "The list";
export const harnessIntro =
  "Ids are stable and lower-case; display names follow each product's own branding. The table comes from the shared harness data, which is tested against the SDK registry.";

export function harnessMarkdown(): string {
  const rows = harnesses.map(
    (h, i) =>
      `| ${i + 1} | ${h.name} | \`${h.id}\` | ${h.fields.join(", ") || "-"} | ${h.estimated ? "estimated" : "reported"} |`,
  );
  return `## ${harnessHeading}\n\n${harnessIntro}\n\n| # | Harness | id | Fields in fixture | Counts |\n| --- | --- | --- | --- | --- |\n${rows.join("\n")}\n\nDetected but not read: ${unsupported.join(", ")}.\n\n`;
}

export const pageMarkdown = (entry: {
  id: string;
  body?: string;
  data: { title: string; description: string };
}) =>
  `# ${entry.data.title}\n\n${entry.data.description}\n\n${entry.id === "reference/harnesses" ? harnessMarkdown() : ""}${entry.body ?? ""}`;
