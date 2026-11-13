(() => {
  const root = document.documentElement;
  const browserColor = document.querySelector('meta[name="theme-color"]');
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)");
  const storageKey = "lovelace-theme";
  let preference;
  let toggle;

  try {
    const saved = localStorage.getItem(storageKey);
    if (saved === "light" || saved === "dark") preference = saved;
  } catch {
    // The toggle still works when the browser blocks storage.
  }

  function applyTheme() {
    const theme = preference ?? (systemTheme.matches ? "dark" : "light");
    root.dataset.theme = theme;
    browserColor.content = theme === "dark" ? "#191416" : "#f8f6f0";
    toggle?.setAttribute("aria-pressed", String(theme === "dark"));
  }

  applyTheme();
  systemTheme.addEventListener("change", applyTheme);

  document.addEventListener("DOMContentLoaded", () => {
    toggle = document.querySelector(".theme-toggle");
    applyTheme();
    toggle.hidden = false;
    toggle.addEventListener("click", () => {
      preference = root.dataset.theme === "dark" ? "light" : "dark";
      applyTheme();
      try {
        localStorage.setItem(storageKey, preference);
      } catch {
        // The current page keeps the selected theme in memory.
      }
    });
  });
})();
