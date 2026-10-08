export function initCopyCode(label: string, root: ParentNode = document): void {
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-copy-code]")) {
    button.hidden = false;
    button.setAttribute("aria-label", label);
    button.title = label;
    if (button.dataset.bound) continue;
    button.dataset.bound = "true";
    button.addEventListener("click", async () => {
      const code = button.closest(".code-frame")?.querySelector("pre")?.textContent ?? "";
      try {
        await navigator.clipboard.writeText(code);
        button.dataset.state = "copied";
      } catch {
        button.dataset.state = "failed";
      }
      setTimeout(() => delete button.dataset.state, 1600);
    });
  }
}
