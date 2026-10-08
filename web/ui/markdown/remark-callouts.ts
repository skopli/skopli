import type { Paragraph, Root } from "mdast";
import type { ContainerDirective, LeafDirective, TextDirective } from "mdast-util-directive";
import type { VFile } from "vfile";
import { visit } from "unist-util-visit";

export const calloutTypes = ["note", "tip", "caution", "warning"] as const;
export type CalloutType = (typeof calloutTypes)[number];

export const calloutTitles: Record<CalloutType, string> = {
  note: "Note",
  tip: "Tip",
  caution: "Caution",
  warning: "Warning",
};

/** Stray `:word` runs (like `T00:30Z`) parse as directives; put the source text back. */
export function restoreStrayDirectives(tree: Root, source: string): void {
  visit(tree, ["textDirective", "leafDirective"], (node, index, parent) => {
    const { position } = node as TextDirective | LeafDirective;
    if (!parent || index === undefined || !position) return;
    parent.children.splice(index, 1, {
      type: "text",
      value: source.slice(position.start.offset, position.end.offset),
    });
  });
}

export function remarkCallouts() {
  return (tree: Root, file: VFile) => {
    restoreStrayDirectives(tree, String(file.value));
    visit(tree, "containerDirective", (node: ContainerDirective) => {
      if (!(calloutTypes as readonly string[]).includes(node.name)) return;
      const type = node.name as CalloutType;
      const label = node.children.find(
        (c) => c.type === "paragraph" && c.data?.directiveLabel === true,
      );
      const title = label && label.type === "paragraph" ? label : undefined;
      const body = node.children.filter((c) => c !== label);
      node.data = {
        ...node.data,
        hName: "aside",
        hProperties: { className: ["callout"], "data-type": type },
      };
      const heading: Paragraph = {
        type: "paragraph",
        data: {
          hName: "strong",
          hProperties: { className: ["callout__title"] },
        } as Paragraph["data"],
        children: title?.children ?? [{ type: "text", value: calloutTitles[type] }],
      };
      node.children = [heading, ...body];
    });
  };
}
