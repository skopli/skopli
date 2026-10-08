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
