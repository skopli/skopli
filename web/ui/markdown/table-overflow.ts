export function initTableOverflow(root: ParentNode = document): void {
  for (const frame of root.querySelectorAll<HTMLElement>(".table-frame")) {
    const scroll = frame.querySelector<HTMLElement>(".table-scroll");
    if (!scroll) continue;
    const update = () => {
      const overflow = scroll.scrollWidth > scroll.clientWidth + 1;
      frame.dataset.overflow = String(overflow);
      frame.dataset.scrolledEnd = String(
        !overflow || scroll.scrollLeft + scroll.clientWidth >= scroll.scrollWidth - 1,
      );
    };
    update();
    scroll.addEventListener("scroll", update, { passive: true });
    new ResizeObserver(update).observe(scroll);
  }
}
