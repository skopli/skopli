export const posthogKey = "phc_xFoaFpDvwWHvF2wuoUUa6cms8kmBR7NfgCMWyWHT72YL";
export const posthogHost = "https://eu.i.posthog.com";

export const pageActionEvents = {
  copyMarkdown: "docs_copy_md",
  chatgpt: "open_chatgpt",
  claude: "open_claude",
  grok: "open_grok",
  perplexity: "open_perplexity",
  googleAiMode: "open_google_ai_mode",
  cursor: "open_cursor",
} as const;

export type PageActionEvent = (typeof pageActionEvents)[keyof typeof pageActionEvents];

/** AI assistants a page can be opened in; `url` takes the URL-encoded prompt. */
export const aiTools = [
  {
    name: "ChatGPT",
    event: pageActionEvents.chatgpt,
    url: (q: string) => `https://chatgpt.com/?q=${q}`,
  },
  {
    name: "Claude",
    event: pageActionEvents.claude,
    url: (q: string) => `https://claude.ai/new?q=${q}`,
  },
  { name: "Grok", event: pageActionEvents.grok, url: (q: string) => `https://grok.com/?q=${q}` },
  {
    name: "Perplexity",
    event: pageActionEvents.perplexity,
    url: (q: string) => `https://perplexity.ai/search?q=${q}`,
  },
  {
    name: "Google AI Mode",
    event: pageActionEvents.googleAiMode,
    url: (q: string) => `https://google.com/search?udm=50&q=${q}`,
  },
  {
    name: "Cursor",
    event: pageActionEvents.cursor,
    url: (q: string) => `https://cursor.com/link/prompt?text=${q}`,
  },
] as const;
