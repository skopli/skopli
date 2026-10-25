import inter400 from "@fontsource/inter/files/inter-latin-400-normal.woff?inline";
import inter500 from "@fontsource/inter/files/inter-latin-500-normal.woff?inline";
import satori from "satori";
import sharp from "sharp";
import mark from "./assets/skopli-mark-dark.svg?raw";
import tokensCss from "./styles/tokens.css?raw";

export const ogSize = { width: 1200, height: 630 } as const;

const rootTokens = tokensCss.match(/:root\s*{([^}]*)}/)?.[1] ?? "";
const token = (name: string): string => {
  const match = rootTokens.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match?.[1]) throw new Error(`token --${name} missing from the :root block of tokens.css`);
  return match[1].trim();
};

const fontData = (dataUrl: string): Buffer => Buffer.from(dataUrl.split(",")[1] ?? "", "base64");

const fonts = [
  { name: "Inter", data: fontData(inter400), weight: 400 as const },
  { name: "Inter", data: fontData(inter500), weight: 500 as const },
];

type Node = { type: string; props: Record<string, unknown> };
const el = (
  type: string,
  style: Record<string, string | number>,
  children?: Node[] | string,
): Node => ({
  type,
  props: { style, children },
});

export type OgCard = { site: string; title: string; description: string; url: string };

export const ogImage = async ({ site, title, description, url }: OgCard): Promise<Uint8Array> => {
  const paper = token("paper");
  const ink = token("ink");
  const ink2 = token("ink-2");
  const ink3 = token("ink-3");
  const rule = token("rule");
  const card = el(
    "div",
    {
      display: "flex",
      flexDirection: "column",
      justifyContent: "space-between",
      width: ogSize.width,
      height: ogSize.height,
      boxSizing: "border-box",
      padding: "64px 72px",
      background: paper,
      color: ink,
      fontFamily: "Inter",
    },
    [
      el("div", { display: "flex", alignItems: "center", gap: 16, fontSize: 28, color: ink2 }, [
        {
          type: "img",
          props: {
            src: `data:image/svg+xml;utf8,${encodeURIComponent(mark)}`,
            width: 40,
            height: 40,
          },
        },
        el("span", {}, site),
      ]),
      el("div", { display: "flex", flexDirection: "column", gap: 24 }, [
        el(
          "div",
          { fontSize: 60, fontWeight: 500, lineHeight: 1.1, letterSpacing: "-0.025em" },
          title,
        ),
        el("div", { fontSize: 28, lineHeight: 1.4, color: ink2 }, description),
      ]),
      el(
        "div",
        {
          display: "flex",
          paddingTop: 24,
          borderTop: `1px solid ${rule}`,
          fontSize: 24,
          color: ink3,
        },
        url,
      ),
    ],
  );
  const svg = await satori(card, { ...ogSize, fonts });
  return new Uint8Array(await sharp(Buffer.from(svg)).png().toBuffer());
};
