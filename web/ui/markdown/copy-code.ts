export interface CopyLabels {
  copy: string;
  copied: string;
  failed: string;
}

/** Announces copy feedback through one polite live region shared by every copy button on the page. */
export function announce(text: string): void {
  let status = document.getElementById("copy-status");
  if (!status) {
    status = document.createElement("div");
    status.id = "copy-status";
    status.className = "visually-hidden";
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    document.body.append(status);
  }
  status.textContent = "";
  requestAnimationFrame(() => {
    status.textContent = text;
  });
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
        announce(labels.copied);
      } catch {
        button.dataset.state = "failed";
        label(labels.failed);
        announce(labels.failed);
      }
      setTimeout(() => {
        delete button.dataset.state;
        label(labels.copy);
      }, 1600);
    });
  }
}
