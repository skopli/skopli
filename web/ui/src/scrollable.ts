export const syncScrollable = (box: HTMLElement) => {
  const sync = () => {
    box.toggleAttribute("data-scrollable", box.scrollWidth > box.clientWidth);
    box.toggleAttribute("data-scrolled", box.scrollLeft + box.clientWidth >= box.scrollWidth - 1);
  };
  box.addEventListener("scroll", sync, { passive: true });
  new ResizeObserver(sync).observe(box);
};
