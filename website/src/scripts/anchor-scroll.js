/**
 * Re-runs the browser's jump to `location.hash` once the page has settled.
 *
 * The browser scrolls to the anchor before the web font finishes loading, so
 * swapping the font in reflows the content out from under the target and leaves
 * the heading scrolled off the top of the viewport.
 */
export function restoreAnchorScroll() {
  const target = getHashTarget();
  if (target == null || document.fonts == null) {
    return;
  }

  const scrollYBefore = window.scrollY;
  document.fonts.ready.then(() => {
    // don't yank the page around if the reader already scrolled somewhere else
    if (window.scrollY === scrollYBefore && getHashTarget() === target) {
      target.scrollIntoView();
    }
  });
}

function getHashTarget() {
  if (location.hash.length <= 1) {
    return null;
  }
  return document.getElementById(decodeURIComponent(location.hash.slice(1)));
}
