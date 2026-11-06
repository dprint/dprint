// filters the plugins page's plugins as the search is typed in, showing the
// matching plugins as one list instead of in their sections
export function setupPluginSearch() {
  const search = document.getElementById("plugin-search");
  const groups = document.getElementById("plugin-groups");
  const empty = document.getElementById("plugin-search-empty");
  if (search == null || groups == null) {
    return; // not the plugins page
  }

  const cards = groups.querySelectorAll(".plugin-card");
  search.addEventListener("input", filter);
  // the browser may restore the search when going back to the page
  filter();

  function filter() {
    const query = search.value.trim().toLowerCase();
    let matches = 0;
    for (const card of cards) {
      const isMatch = query === "" || (card.dataset.search ?? "").includes(query);
      card.hidden = !isMatch;
      if (isMatch) {
        matches++;
      }
    }
    groups.classList.toggle("searching", query !== "");
    groups.hidden = matches === 0;
    if (empty != null) {
      empty.hidden = matches !== 0;
    }
  }
}
