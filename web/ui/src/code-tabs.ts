import type { Element, Root } from "hast";
import type { ShikiTransformer } from "shiki";

// Fenced blocks written as ```ts tab=quickstart are grouped, when adjacent, into one
// tab set that follows the site-wide language switch. The Markdown twin keeps them in sequence.
export const tabMetaTransformer: ShikiTransformer = {
  name: "skopli-tab-meta",
  pre(node) {
    const tab = /\btab=([\w-]+)/.exec(this.options.meta?.__raw ?? "")?.[1];
    if (tab) node.properties["data-tab"] = tab;
  },
};

const isTabbedPre = (n: Root["children"][number] | Element["children"][number]): boolean =>
  n.type === "element" && n.tagName === "pre" && "data-tab" in n.properties;

export function rehypeCodeTabs() {
  return (tree: Root) => {
    const visit = (parent: Root | Element) => {
      const out: (typeof parent.children)[number][] = [];
      let group: Element[] = [];
      const flush = () => {
        if (group.length) out.push(wrap(group));
        group = [];
      };
      for (const child of parent.children) {
        if (isTabbedPre(child)) {
          group.push(child as Element);
          continue;
        }
        if (child.type === "text" && /^\s*$/.test(child.value) && group.length) continue;
        flush();
        if (child.type === "element") visit(child);
        out.push(child);
      }
      flush();
      parent.children = out as typeof parent.children;
    };
    visit(tree);
  };
}

function wrap(pres: Element[]): Element {
  return {
    type: "element",
    tagName: "div",
    properties: { className: ["code-tabs"], "data-tab": pres[0]?.properties["data-tab"] },
    children: pres.map((pre) => ({
      type: "element",
      tagName: "div",
      properties: {
        className: ["code-tab"],
        "data-lang": pre.properties.dataLanguage ?? pre.properties["data-language"],
        role: "tabpanel",
      },
      children: [pre],
    })),
  };
}
