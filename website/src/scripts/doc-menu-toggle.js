// the documentation sidebar menu is open by default (for desktop); collapse it
// on small screens so it doesn't push the page content far down.
export function setupDocMenu() {
  const details = document.querySelector("details.doc-nav");
  if (details == null) {
    return; // not a documentation page
  }
  if (window.matchMedia("(max-width: 860px)").matches) {
    details.open = false;
  }
}
