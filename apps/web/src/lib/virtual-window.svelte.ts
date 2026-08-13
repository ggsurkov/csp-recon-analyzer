/**
 * Fixed-height row windowing.
 *
 * Keeps a scroll container's visible index range in reactive state so a list of any length
 * can render only the rows on screen. The scrollbar stays honest because the rows that are
 * *not* rendered are replaced by spacers of exactly their combined height — the document is
 * the full size, only the DOM is small.
 *
 * ## Why the row height has to be a constant
 *
 * Mapping `scrollTop` to an index is a division. That only works if every row is the same
 * height, which means the *content* must be incapable of growing a row — clamped text, no
 * wrapping headlines. Measuring real heights instead would mean rendering rows to find out
 * how tall they are, which is the thing this exists to avoid. A caller that cannot clamp
 * its rows should not use this.
 *
 * ## Scroll handling
 *
 * The listener is passive and does nothing but request a frame: reading `scrollTop` inside
 * the scroll event forces layout on the same thread the browser is trying to scroll on.
 * Coalescing into `requestAnimationFrame` means at most one measurement per frame however
 * many events fire, and none at all on frames the browser skips.
 */
export class VirtualWindow {
  /**
   * Height of one row in CSS pixels. Must match the rendered row exactly.
   *
   * Declared with a default rather than assigned only in the constructor: class field
   * initializers all run before the constructor body, so the `$derived` fields below would
   * be reading an undefined `rowHeight` at declaration time. Giving it a real default keeps
   * the derivations well-formed, and the constructor overwrites it before anything reads.
   */
  rowHeight = $state(48);
  /** Rows to render beyond each edge, so a fast flick cannot expose a gap. */
  overscan = $state(5);

  /** Live scroll offset of the container, updated once per animation frame. */
  scrollTop = $state(0);
  /** Visible height of the container, tracked through a ResizeObserver. */
  viewportHeight = $state(0);
  /** Total rows in the backing list. */
  count = $state(0);

  constructor(rowHeight = 48, overscan = 5) {
    if (rowHeight <= 0) throw new Error('rowHeight must be positive');
    this.rowHeight = rowHeight;
    this.overscan = overscan;
  }

  /** Index of the first rendered row. */
  readonly start = $derived(
    Math.max(0, Math.floor(this.scrollTop / this.rowHeight) - this.overscan)
  );

  /** Index one past the last rendered row. */
  readonly end = $derived(
    Math.min(
      this.count,
      Math.ceil((this.scrollTop + this.viewportHeight) / this.rowHeight) + this.overscan
    )
  );

  /** Height of the spacer standing in for the rows above the window. */
  readonly padTop = $derived(this.start * this.rowHeight);

  /** Height of the spacer standing in for the rows below it. */
  readonly padBottom = $derived(Math.max(0, this.count - this.end) * this.rowHeight);

  /**
   * Bind to the scrolling element. Returns a teardown function, so it is used as the whole
   * body of an `$effect`.
   */
  observe(node: HTMLElement): () => void {
    let frame = 0;

    const sample = () => {
      frame = 0;
      this.scrollTop = node.scrollTop;
    };

    const onScroll = () => {
      if (frame) return;
      frame = requestAnimationFrame(sample);
    };

    const resize = new ResizeObserver(() => {
      this.viewportHeight = node.clientHeight;
    });

    node.addEventListener('scroll', onScroll, { passive: true });
    resize.observe(node);

    // Seed both, or the first paint renders an empty window and flashes.
    this.viewportHeight = node.clientHeight;
    this.scrollTop = node.scrollTop;

    return () => {
      node.removeEventListener('scroll', onScroll);
      resize.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }
}
