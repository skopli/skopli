import type { Element, Root } from "hast";
import type { ShikiTransformer } from "shiki";

const copyIcon: Element = {
  type: "element",
  tagName: "svg",
  properties: {
    width: "15",
    height: "15",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2",
    strokeLinecap: "round",
    strokeLinejoin: "round",
    ariaHidden: "true",
  },
  children: [
    {
      type: "element",
      tagName: "rect",
      properties: { x: "9", y: "9", width: "13", height: "13", rx: "2" },
      children: [],
    },
    {
      type: "element",
      tagName: "path",
      properties: { d: "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" },
      children: [],
    },
  ],
};

function titleFromMeta(meta: string | undefined): string | undefined {
  return meta
    ? /(?:^|\s)title=(?:"([^"]*)"|'([^']*)'|(\S+))/.exec(meta)?.slice(1).find(Boolean)
    : undefined;
}

export function shikiFrame(): ShikiTransformer {
  return {
    name: "skopli-code-frame",
    root(root: Root) {
      const pre = root.children.find(
        (n): n is Element => n.type === "element" && n.tagName === "pre",
      );
      if (!pre) return;
      const title = titleFromMeta(this.options.meta?.__raw);
      const copy: Element = {
        type: "element",
        tagName: "button",
        properties: {
          type: "button",
          className: ["code-frame__copy"],
          "data-copy-code": "",
          hidden: true,
        },
        children: [copyIcon],
      };
      const head: Element = {
        type: "element",
        tagName: "div",
        properties: { className: ["code-frame__head"] },
        children: [
          {
            type: "element",
            tagName: "span",
            properties: { className: ["code-frame__title"] },
            children: [{ type: "text", value: title ?? "" }],
          },
          copy,
        ],
      };
      const frame: Element = {
        type: "element",
        tagName: "figure",
        properties: {
          className: ["code-frame", title ? "code-frame--titled" : "code-frame--untitled"],
          dataPagefindIgnore: "",
        },
        children: title ? [head, pre] : [pre, copy],
      };
      return { ...root, children: [frame] };
    },
  };
}
