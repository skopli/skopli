import type { Element, Root } from "hast";
import { visit } from "unist-util-visit";

export function rehypeTableFrame() {
  return (tree: Root) => {
    visit(tree, "element", (node: Element, index, parent) => {
      if (node.tagName !== "table" || !parent || index === undefined) return;
      if (
        parent.type === "element" &&
        parent.properties.className?.toString().includes("table-scroll")
      )
        return;
      const frame: Element = {
        type: "element",
        tagName: "div",
        properties: { className: ["table-frame"] },
        children: [
          {
            type: "element",
            tagName: "div",
            properties: { className: ["table-scroll"], tabIndex: 0 },
            children: [node],
          },
        ],
      };
      parent.children[index] = frame;
    });
  };
}
