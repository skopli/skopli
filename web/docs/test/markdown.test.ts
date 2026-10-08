import { describe, expect, it } from "vitest";
import { toMarkdown } from "../src/lib/markdown.ts";

const siteBase = "https://docs.skopli.com/skopli";
const page = (body: string) =>
  toMarkdown({ title: "T", description: "D", body, siteBase, source: "test" });

describe("toMarkdown", () => {
  it("drops imports and flattens tabs into labelled sections", () => {
    const md = page(`import Tabs from "@skopli/ui/components/Tabs.astro";

<Tabs syncKey="lang">
  <Tab label="TypeScript">
    \`\`\`ts
    const a = 1;
    \`\`\`

  </Tab>
  <Tab label="Python">
    \`\`\`python
    a = 1
    \`\`\`

  </Tab>
</Tabs>
`);
    expect(md).not.toContain("import ");
    expect(md).toContain("**TypeScript**\n\n```ts\nconst a = 1;\n```");
    expect(md).toContain("**Python**\n\n```python\na = 1\n```");
  });

  it("turns link cards into a link list and absolutizes relative links", () => {
    const md = page(`<LinkList>
  <LinkCard title="Read local logs" href="/skopli/guide/reading">
    Options and fields.
  </LinkCard>
</LinkList>

See [Rollups](/skopli/guide/rollups#billing-blocks).
`);
    expect(md).toContain(
      "- [Read local logs](https://docs.skopli.com/skopli/guide/reading): Options and fields.",
    );
    expect(md).toContain("[Rollups](https://docs.skopli.com/skopli/guide/rollups#billing-blocks)");
  });

  it("renders callouts as titled blockquotes and keeps footnotes", () => {
    const md = page(`:::caution[One documented assumption]
Reasoning tokens may be double-billed.[^a]
:::

[^a]: The footnote body.
`);
    expect(md).toContain(
      "> **One documented assumption**\n>\n> Reasoning tokens may be double-billed.[^a]",
    );
    expect(md).toContain("[^a]: The footnote body.");
  });

  it("replaces harness components with registry names", () => {
    const md = page(`| Harness | id |
| --- | --- |
| <HarnessName id="copilot" /> | \`copilot\` |
| <HarnessIcon slug="goose" />goose | \`goose\` |
`);
    expect(md).toContain("| GitHub Copilot CLI | `copilot` |");
    expect(md).toContain("| goose              | `goose`   |");
  });

  it("throws on unknown JSX and unknown directives", () => {
    expect(() => page(`<Mystery />`)).toThrow(/unknown MDX element <Mystery>/);
    expect(() => page(`:::danger\nx\n:::`)).toThrow(/unknown directive :::danger/);
  });
});
