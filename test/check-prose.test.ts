import { describe, expect, it } from "vitest";
import { paragraphLines, stripComments, twoSentences } from "../scripts/check-prose.mjs";

const doc = [
  "# Title",
  "",
  "First line. Second sentence here.",
  "<!-- a note. Spanning",
  "two lines. Still hidden. -->Visible after. Also visible.",
  "A `<!-` remnant is prose. It is checked.",
  "<!-- open to the end. Never closed.",
  "Hidden. Also hidden.",
].join("\n");

describe("check-prose", () => {
  it("blanks comments across lines and keeps every newline", () => {
    const stripped = stripComments(doc);
    expect(stripped.split("\n").length).toBe(doc.split("\n").length);
    expect(stripped).not.toContain("hidden");
    expect(stripped).toContain("<!-");
    expect(stripped).toContain("Visible after. Also visible.");
  });

  it("reports the original line numbers around multi-line comments", () => {
    const flagged = paragraphLines(doc)
      .filter(([line]) => twoSentences(line))
      .map(([, n]) => n);
    expect(flagged).toEqual([3, 5, 6]);
  });
});
