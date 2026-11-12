// Use the public repository URL and the published archive URLs for each platform.
const repositoryLinks = {
  source: "",
  windows: "",
  linux: "",
};

for (const link of document.querySelectorAll("[data-repository-link]")) {
  const key = link.dataset.repositoryLink;
  const url = repositoryLinks[key].trim();
  if (!url) continue;

  link.href = url;
  link.hidden = false;
  document.querySelector(`[data-link-placeholder="${key}"]`).hidden = true;
}
