// drives the install command tabs + copy button on the home page
const commands = {
  shell: "curl -fsSL https://dprint.dev/install.sh | sh",
  pwsh: "irm https://dprint.dev/install.ps1 | iex",
  npm: "npm install -g dprint",
  brew: "brew install dprint",
  cargo: "cargo install --locked dprint",
};

export function addInstallTabsEvent() {
  const tabs = document.querySelectorAll(".os-tab");
  const cmdText = document.getElementById("cmd-text");
  const copyBtn = document.getElementById("copy-btn");
  if (tabs.length === 0 || cmdText == null) {
    return; // not on the home page
  }

  tabs.forEach(function(tab) {
    tab.addEventListener("click", function() {
      tabs.forEach(function(t) {
        t.classList.remove("active");
      });
      tab.classList.add("active");
      const os = tab.getAttribute("data-os");
      if (commands[os] != null) {
        cmdText.textContent = commands[os];
      }
      if (copyBtn != null) {
        copyBtn.textContent = "copy";
      }
    });
  });

  if (copyBtn != null) {
    let copyTimeout;
    copyBtn.addEventListener("click", function() {
      try {
        if (navigator.clipboard != null) {
          navigator.clipboard.writeText(cmdText.textContent);
        }
      } catch (err) {
        // ignore
      }
      copyBtn.textContent = "copied ✓";
      clearTimeout(copyTimeout);
      copyTimeout = setTimeout(function() {
        copyBtn.textContent = "copy";
      }, 1600);
    });
  }
}
