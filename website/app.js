const waveform = document.querySelector(".waveform");

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

const copyStatus = document.querySelector(".copy-status");

for (const button of document.querySelectorAll(".copy-command")) {
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
