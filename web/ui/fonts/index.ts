import geistSans from "@fontsource-variable/geist/files/geist-latin-wght-normal.woff2?url";
import geistMono from "@fontsource-variable/geist-mono/files/geist-mono-latin-wght-normal.woff2?url";

export const fontFiles = [geistSans, geistMono];

export const fontFaces = `
@font-face {
  font-family: "Geist Variable";
  font-style: normal;
  font-weight: 100 900;
  font-display: swap;
  src: url(${geistSans}) format("woff2-variations");
  unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD;
}
@font-face {
  font-family: "Geist Mono Variable";
  font-style: normal;
  font-weight: 100 900;
  font-display: swap;
  src: url(${geistMono}) format("woff2-variations");
  unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD;
}
@font-face {
  font-family: "Geist Fallback";
  src: local("Arial"), local("Liberation Sans");
  size-adjust: 107.16%;
  ascent-override: 93.79%;
  descent-override: 27.53%;
  line-gap-override: 0%;
}
@font-face {
  font-family: "Geist Mono Fallback";
  src: local("Courier New"), local("Liberation Mono");
  size-adjust: 99.98%;
  ascent-override: 100.52%;
  descent-override: 29.5%;
  line-gap-override: 0%;
}
`;
