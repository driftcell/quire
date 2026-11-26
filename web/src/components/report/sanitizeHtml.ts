const BLOCKED_HTML_TAGS =
  "script, iframe, object, embed, base, meta, link, noscript";
const URL_ATTRIBUTES = new Set([
  "href",
  "src",
  "xlink:href",
  "action",
  "formaction",
  "poster",
  "data",
]);

function normalizeUrlProtocol(raw: string): string {
  return Array.from(raw)
    .filter((char) => {
      const code = char.charCodeAt(0);
      return code > 0x20 && code !== 0x7f;
    })
    .join("")
    .toLowerCase();
}

/**
 * Strip executable markup from agent-authored report/cover HTML before it
 * reaches `innerHTML`. Script-capable elements, inline event handlers, `srcdoc`,
 * and `javascript:` / `vbscript:` / `data:text/html` URLs are removed while
 * ordinary layout markup is preserved.
 */
export function sanitizeReportHtml(raw: string): string {
  const doc = new DOMParser().parseFromString(raw, "text/html");
  for (const element of Array.from(doc.querySelectorAll(BLOCKED_HTML_TAGS))) {
    element.remove();
  }
  for (const element of Array.from(doc.body.querySelectorAll("*"))) {
    const tag = element.tagName.toLowerCase();
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith("on") || name === "srcdoc") {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (
        (tag === "set" || tag === "animate" || tag === "animatemotion") &&
        name === "attributename" &&
        attribute.value.trim().toLowerCase().startsWith("on")
      ) {
        element.remove();
        break;
      }
      if (!URL_ATTRIBUTES.has(name)) {
        continue;
      }
      const value = normalizeUrlProtocol(attribute.value);
      if (
        value.startsWith("javascript:") ||
        value.startsWith("vbscript:") ||
        value.startsWith("data:text/html")
      ) {
        element.removeAttribute(attribute.name);
      }
    }
  }
  return doc.body.innerHTML;
}
