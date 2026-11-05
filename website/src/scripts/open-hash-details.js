/**
 * Opens the collapsed sections (`<details>`) that contain the element the url
 * links to, so linking to one of a plugin configuration property's AST node
 * specific properties shows it.
 */
export function openHashDetails() {
  openDetailsOfHashTarget();
  window.addEventListener("hashchange", openDetailsOfHashTarget);
}

function openDetailsOfHashTarget() {
  if (location.hash.length <= 1) {
    return;
  }
  const target = document.getElementById(decodeURIComponent(location.hash.slice(1)));
  let details = target?.closest("details");
  if (details == null || details.open) {
    return;
  }
  while (details != null) {
    details.open = true;
    details = details.parentElement?.closest("details");
  }
  target.scrollIntoView();
}
