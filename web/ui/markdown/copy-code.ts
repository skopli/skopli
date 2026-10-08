export interface CopyLabels {
  copy: string;
  copied: string;
  failed: string;
}

export function initCopyCode(labels: CopyLabels, root: ParentNode = document): void {
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-copy-code]")) {
    button.hidden = false;
    const label = (text: string) => {
      button.setAttribute("aria-label", text);
      button.title = text;
    };
    label(labels.copy);
    if (button.dataset.bound) continue;
    button.dataset.bound = "true";
    button.addEventListener("click", async () => {
      const code = button.closest(".code-frame")?.querySelector("pre")?.textContent ?? "";
      try {
        await navigator.clipboard.writeText(code);
        button.dataset.state = "copied";
        label(labels.copied);
      } catch {
        button.dataset.state = "failed";
        label(labels.failed);
      }
      setTimeout(() => {
        delete button.dataset.state;
        label(labels.copy);
      }, 1600);
    });
  }
}
