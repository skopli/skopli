/** Marks `.table-frame` and `.code-frame` with `data-overflow` and `data-scrolled-end` so CSS can fade the clipped edge. */
export function initScrollOverflow(root: ParentNode = document): void {
  for (const frame of root.querySelectorAll<HTMLElement>(".table-frame, .code-frame")) {
    const scroll = frame.querySelector<HTMLElement>(".table-scroll, pre");
    if (!scroll) continue;
    const update = () => {
      const overflow = scroll.scrollWidth > scroll.clientWidth + 1;
      frame.dataset.overflow = String(overflow);
      if (scroll.classList.contains("table-scroll")) {
        if (overflow) scroll.tabIndex = 0;
        else scroll.removeAttribute("tabindex");
      }
      frame.dataset.scrolledEnd = String(
        !overflow || scroll.scrollLeft + scroll.clientWidth >= scroll.scrollWidth - 1,
      );
    };
    update();
    scroll.addEventListener("scroll", update, { passive: true });
    const observer = new ResizeObserver(update);
    observer.observe(scroll);
    if (scroll.firstElementChild) observer.observe(scroll.firstElementChild);
  }
}
