import type { Element, Root } from "hast";
import { visit } from "unist-util-visit";
import type { VFile } from "vfile";
import { localeOfPath, translator } from "../i18n/index.ts";

const headings = new Set(["h2", "h3", "h4"]);

export function rehypeHeadingAnchors() {
  return (tree: Root, file: VFile) => {
    const t = translator(localeOfPath(file.path));
    visit(tree, "element", (node: Element) => {
      if (!headings.has(node.tagName) || typeof node.properties.id !== "string") return;
      if (node.children.some((c) => c.type === "element" && c.tagName === "a")) return;
      node.children.push({
        type: "element",
        tagName: "a",
        properties: {
          className: ["heading-anchor"],
          href: `#${node.properties.id}`,
          ariaLabel: t("sectionLink"),
        },
        children: [{ type: "text", value: "#" }],
      });
    });
  };
}
