/**
 * Keeps the `--nav-h` custom property in sync with the sticky nav's real height.
 *
 * The stylesheet uses it for `scroll-padding-top` so that linking to a heading
 * (ex. /cli/#formatting-a-list-of-files-from-standard-input) doesn't leave the
 * heading hidden underneath the nav. A static value can't work because the nav
 * wraps to two or three rows on narrow screens.
 */
export function setupNavHeight() {
  const nav = document.querySelector(".site-nav");
  if (nav == null) {
    return;
  }

  update();

  if (typeof ResizeObserver !== "undefined") {
    new ResizeObserver(update).observe(nav);
  } else {
    window.addEventListener("resize", update);
  }

  function update() {
    const height = Math.round(nav.getBoundingClientRect().height);
    document.documentElement.style.setProperty("--nav-h", `${height}px`);
  }
}
