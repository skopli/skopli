import type { Element, ElementContent, Root } from "hast";
import { visit } from "unist-util-visit";
import type { VFile } from "vfile";
import { localeOfPath, translator } from "../i18n/index.ts";

const text = (node: ElementContent): string =>
  node.type === "text"
    ? node.value
    : node.type === "element"
      ? node.children.map(text).join("")
      : "";

export function rehypeTableFrame() {
  return (tree: Root, file: VFile) => {
    const t = translator(localeOfPath(file.path));
    visit(tree, "element", (node: Element, index, parent) => {
      if (node.tagName !== "table" || !parent || index === undefined) return;
      if (
        parent.type === "element" &&
        parent.properties.className?.toString().includes("table-scroll")
      )
        return;
      const caption = node.children.find((c) => c.type === "element" && c.tagName === "caption");
      const label = caption ? text(caption).trim() : t("table");
      const frame: Element = {
        type: "element",
        tagName: "div",
        properties: { className: ["table-frame"] },
        children: [
          {
            type: "element",
            tagName: "div",
            properties: { className: ["table-scroll"], role: "region", ariaLabel: label },
            children: [node],
          },
        ],
      };
      parent.children[index] = frame;
    });
  };
}
