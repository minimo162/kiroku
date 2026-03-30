function escapeHtml(text: string) {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function escapeRegExp(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function highlightText(text: string, query: string | null | undefined) {
  const normalizedQuery = query?.trim();
  if (!normalizedQuery) {
    return escapeHtml(text);
  }

  const regex = new RegExp(escapeRegExp(normalizedQuery), "gi");
  let result = "";
  let lastIndex = 0;

  for (const match of text.matchAll(regex)) {
    const index = match.index ?? 0;
    const matchedText = match[0];
    result += escapeHtml(text.slice(lastIndex, index));
    result += `<mark class="rounded px-0.5 bg-brass-200">${escapeHtml(matchedText)}</mark>`;
    lastIndex = index + matchedText.length;
  }

  result += escapeHtml(text.slice(lastIndex));
  return result;
}
