import type { Element, Root } from "hast";
import type { ShikiTransformer } from "shiki";
import { languages } from "./data/languages.ts";

const switchId = (shiki: string) => languages.find((l) => l.shiki === shiki)?.id;

// Adjacent fences written as ```ts tab=quickstart form one tab set that follows the site-wide
// language switch; `lang=py` pins a fence (an install line in sh, say) to a switch id.
export const tabMetaTransformer: ShikiTransformer = {
  name: "skopli-tab-meta",
  pre(node) {
    const meta = this.options.meta?.__raw ?? "";
    const tab = /\btab=([\w-]+)/.exec(meta)?.[1];
    if (!tab) return;
    node.properties["data-tab"] = tab;
    node.properties["data-switch"] =
      /\blang=([\w-]+)/.exec(meta)?.[1] ?? switchId(this.options.lang);
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
        "data-lang": pre.properties["data-switch"],
        role: "tabpanel",
      },
      children: [pre],
    })),
  };
}
