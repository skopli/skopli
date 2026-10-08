import { harnessName } from "@skopli/ui/data/harnesses.ts";
import {
  calloutTitles,
  calloutTypes,
  type CalloutType,
} from "@skopli/ui/markdown/remark-callouts.ts";
import type {
  Blockquote,
  Link,
  ListItem,
  Paragraph,
  PhrasingContent,
  Root,
  RootContent,
} from "mdast";
import type { ContainerDirective } from "mdast-util-directive";
import type { MdxJsxAttribute, MdxJsxFlowElement, MdxJsxTextElement } from "mdast-util-mdx-jsx";
import remarkDirective from "remark-directive";
import remarkGfm from "remark-gfm";
import remarkMdx from "remark-mdx";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import { unified } from "unified";
import { visit } from "unist-util-visit";

export interface MarkdownPage {
  title: string;
  description: string;
  body: string;
  /** Absolute origin plus base, for example `https://docs.skopli.com/skopli`. */
  siteBase: string;
  /** Root-relative page path under the base, for example `/guide/reading`. */
  path: string;
  /** Name shown in errors. */
  source?: string;
}

const parser = unified().use(remarkParse).use(remarkMdx).use(remarkGfm).use(remarkDirective);
const printer = unified().use(remarkGfm).use(remarkStringify, {
  bullet: "-",
  emphasis: "_",
  fences: true,
  rule: "-",
  resourceLink: true,
});

type Jsx = MdxJsxFlowElement | MdxJsxTextElement;

function attr(node: Jsx, name: string): string | undefined {
  const found = node.attributes.find(
    (a): a is MdxJsxAttribute => a.type === "mdxJsxAttribute" && a.name === name,
  );
  return typeof found?.value === "string" ? found.value : undefined;
}

function paragraph(children: PhrasingContent[]): Paragraph {
  return { type: "paragraph", children };
}

function strong(text: string): PhrasingContent {
  return { type: "strong", children: [{ type: "text", value: text }] };
}

function fail(page: MarkdownPage, message: string): never {
  throw new Error(`${page.source ?? "markdown"}: ${message}`);
}

function convertJsx(node: Jsx, page: MarkdownPage): RootContent[] {
  const children = node.children as RootContent[];
  switch (node.name) {
    case "Tabs":
      return children.flatMap((child) =>
        child.type === "mdxJsxFlowElement" && child.name === "Tab"
          ? [
              paragraph([strong(attr(child, "label") ?? fail(page, "<Tab> without label"))]),
              ...convertBlocks(child.children as RootContent[], page),
            ]
          : convertBlocks([child], page),
      );
    case "LinkList":
      return [
        {
          type: "list",
          ordered: false,
          spread: false,
          children: children
            .filter((c) => c.type !== "text" || c.value.trim() !== "")
            .map((card): ListItem => {
              if (card.type !== "mdxJsxFlowElement" || card.name !== "LinkCard")
                fail(page, `<LinkList> child ${card.type} is not a <LinkCard>`);
              const link: Link = {
                type: "link",
                url: attr(card, "href") ?? fail(page, "<LinkCard> without href"),
                children: [
                  {
                    type: "text",
                    value: attr(card, "title") ?? fail(page, "<LinkCard> without title"),
                  },
                ],
              };
              const body = convertBlocks(card.children as RootContent[], page).flatMap((block) =>
                block.type === "paragraph"
                  ? block.children
                  : fail(page, "<LinkCard> body must be text"),
              );
              return {
                type: "listItem",
                spread: false,
                children: [paragraph([link, { type: "text", value: ": " }, ...body])],
              };
            }),
        },
      ];
    case "HarnessName":
      return [
        {
          type: "text",
          value: harnessName(attr(node, "id") ?? fail(page, "<HarnessName> without id")),
        },
      ];
    case "HarnessIcon":
      return [];
    default:
      return fail(page, `unknown MDX element <${node.name ?? "fragment"}>`);
  }
}

/** Stray `:word` runs (like `T00:30Z`) parse as directives; put the source text back. */
function sourceText(node: RootContent, page: MarkdownPage): string {
  const { start, end } = node.position ?? fail(page, "directive without position");
  return page.body.slice(start.offset, end.offset);
}

function convertCallout(node: ContainerDirective, page: MarkdownPage): Blockquote {
  if (!calloutTypes.includes(node.name as CalloutType))
    fail(page, `unknown directive :::${node.name}`);
  const label = node.children.find(
    (c) => c.type === "paragraph" && c.data?.directiveLabel === true,
  );
  const title =
    label?.type === "paragraph"
      ? label.children
      : [{ type: "text" as const, value: calloutTitles[node.name as CalloutType] }];
  return {
    type: "blockquote",
    children: [
      paragraph([{ type: "strong", children: title }]),
      ...(convertBlocks(
        node.children.filter((c) => c !== label) as RootContent[],
        page,
      ) as Blockquote["children"]),
    ],
  };
}

function convertBlocks(nodes: RootContent[], page: MarkdownPage): RootContent[] {
  return nodes.flatMap((node): RootContent[] => {
    switch (node.type) {
      case "mdxjsEsm":
        return [];
      case "mdxFlowExpression":
      case "mdxTextExpression":
        return fail(page, `unsupported expression {${node.value}}`);
      case "mdxJsxFlowElement":
      case "mdxJsxTextElement":
        return convertJsx(node, page);
      case "containerDirective":
        return [convertCallout(node, page)];
      case "leafDirective":
      case "textDirective":
        return [{ type: "text", value: sourceText(node, page) }];
      default:
        if ("children" in node) {
          (node as { children: RootContent[] }).children = convertBlocks(
            node.children as RootContent[],
            page,
          );
        }
        return [node];
    }
  });
}

function absolutize(tree: Root, page: MarkdownPage): void {
  const origin = new URL(page.siteBase).origin;
  const here = `${page.siteBase}${page.path === "/" ? "/" : page.path}`;
  const absolute = (url: string) =>
    url.startsWith("/")
      ? `${origin}${url}`
      : url.startsWith("#")
        ? `${here}${url}`
        : new URL(url, here).href;
  visit(tree, (node) => {
    if (node.type === "link" || node.type === "definition" || node.type === "image")
      node.url = absolute(node.url);
  });
}

/** Convert one MDX docs page into plain Markdown for the `.md` twin and llms outputs. */
export function toMarkdown(page: MarkdownPage): string {
  const tree = parser.parse(page.body) as Root;
  tree.children = convertBlocks(tree.children, page);
  absolutize(tree, page);
  const body = printer.stringify(tree).trim();
  return `# ${page.title}\n\n> ${page.description}\n\n${body}\n`;
}
