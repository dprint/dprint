/**
 * Copies the text of a button's `data-copy` attribute when it's clicked (ex.
 * the command to add a plugin on its page).
 */
export function setupCopyButtons() {
  document.addEventListener("click", function(event) {
    const button = event.target instanceof Element ? event.target.closest("button[data-copy]") : null;
    if (button == null) {
      return;
    }
    if (navigator.clipboard != null) {
      navigator.clipboard.writeText(button.getAttribute("data-copy")).catch(function() {});
    }
    button.textContent = "copied ✓";
    clearTimeout(button.copyTimeout);
    button.copyTimeout = setTimeout(function() {
      button.textContent = "copy";
    }, 1600);
  });
}
