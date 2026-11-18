import { useEffect, useRef } from "react";
import type { CustomHtmlBlock } from "@/contracts/report-protocol";

export interface CustomHtmlViewProps {
  block: CustomHtmlBlock;
}

export function CustomHtmlView({ block }: CustomHtmlViewProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const css = typeof block.css === "string" ? block.css : undefined;
  const html = typeof block.html === "string" ? block.html : "";

  useEffect(() => {
    const host = hostRef.current;
    if (!host) {
      return;
    }
    const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    while (shadow.firstChild) {
      shadow.removeChild(shadow.firstChild);
    }
    if (css) {
      const style = document.createElement("style");
      style.textContent = css;
      shadow.appendChild(style);
    }
    const wrapper = document.createElement("div");
    wrapper.className = "quire-custom-html";
    wrapper.innerHTML = html;
    shadow.appendChild(wrapper);
  }, [html, css]);

  return <div ref={hostRef} className="w-full" />;
}
