import type { Element, Root } from "hast";
import { visit } from "unist-util-visit";

const headings = new Set(["h2", "h3", "h4"]);

export function rehypeHeadingAnchors() {
  return (tree: Root) => {
    visit(tree, "element", (node: Element) => {
      if (!headings.has(node.tagName) || typeof node.properties.id !== "string") return;
      if (node.children.some((c) => c.type === "element" && c.tagName === "a")) return;
      node.children.unshift({
        type: "element",
        tagName: "a",
        properties: {
          className: ["heading-anchor"],
          href: `#${node.properties.id}`,
          ariaLabel: "#",
          tabIndex: -1,
        },
        children: [{ type: "text", value: "#" }],
      });
    });
  };
}
