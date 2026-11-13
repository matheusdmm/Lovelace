const waveform = document.querySelector(".waveform");

const header = document.querySelector(".site-header");
const navigation = header.querySelector("nav");
const navLinks = [...header.querySelectorAll("nav a, .header-cta")];
const sections = [...document.querySelectorAll("main > section[id]")];
let currentSection;
let scrollFrame;

function updateCurrentSection() {
  scrollFrame = undefined;
  const readingLine = header.getBoundingClientRect().bottom + 24;
  const { scrollTop, scrollHeight, clientHeight } = document.scrollingElement;
  const atBottom = scrollTop > 0 && scrollHeight - scrollTop - clientHeight <= 2;
  const nextSection = atBottom
    ? sections.at(-1)
    : sections.findLast((section) => section.getBoundingClientRect().top <= readingLine);
  if (nextSection === currentSection) return;

  currentSection?.classList.remove("is-reading");
  currentSection = nextSection;
  currentSection?.classList.add("is-reading");

  for (const link of navLinks) {
    if (link.hash.slice(1) !== currentSection?.id) {
      link.removeAttribute("aria-current");
      continue;
    }

    link.setAttribute("aria-current", "location");
    if (!navigation.contains(link)) continue;

    const navBounds = navigation.getBoundingClientRect();
    const linkBounds = link.getBoundingClientRect();
    if (linkBounds.left < navBounds.left || linkBounds.right > navBounds.right) {
      navigation.scrollLeft += linkBounds.left - navBounds.left - (navBounds.width - linkBounds.width) / 2;
    }
  }
}

function scheduleSectionUpdate() {
  if (scrollFrame === undefined) scrollFrame = requestAnimationFrame(updateCurrentSection);
}

new ResizeObserver(() => {
  document.documentElement.style.setProperty("--header-height", `${header.offsetHeight}px`);
  scheduleSectionUpdate();
}).observe(header);

window.addEventListener("scroll", scheduleSectionUpdate, { passive: true });
window.addEventListener("resize", scheduleSectionUpdate);
window.addEventListener("pageshow", scheduleSectionUpdate);
scheduleSectionUpdate();

for (let index = 0; index < 111; index += 1) {
  const bar = document.createElement("i");
  const envelope = Math.sin(index * 0.086) ** 2;
  const variation = Math.sin(index * 1.83) ** 2;
  bar.style.setProperty("--height", `${5 + envelope * variation * 89}%`);
  bar.style.setProperty("--opacity", 0.48 + envelope * 0.52);
  bar.style.setProperty("--delay", `${-(index % 17) / 7}s`);
  waveform.append(bar);
}

const platforms = document.querySelectorAll("[data-platform]");

for (const button of platforms) {
  button.addEventListener("click", () => {
    for (const platform of platforms) {
      const selected = platform === button;
      platform.setAttribute("aria-pressed", selected);
      document.getElementById(platform.getAttribute("aria-controls")).hidden = !selected;
    }
  });
}

for (const button of document.querySelectorAll(".copy-command")) {
  const copyStatus = button.closest("section").querySelector(".copy-status");
  let reset;
  button.addEventListener("click", async () => {
    const command = button.previousElementSibling;
    clearTimeout(reset);
    try {
      await navigator.clipboard.writeText(command.textContent);
      button.textContent = "Copied";
      copyStatus.textContent = "Command copied.";
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(command);
      selection.removeAllRanges();
      selection.addRange(range);
      if (document.execCommand("copy")) {
        selection.removeAllRanges();
        button.textContent = "Copied";
        copyStatus.textContent = "Command copied.";
      } else {
        copyStatus.textContent = "Command selected. Press Ctrl+C or Command+C to copy it.";
      }
    }
    reset = setTimeout(() => {
      button.textContent = "Copy";
      copyStatus.textContent = "";
    }, 2500);
  });
}
