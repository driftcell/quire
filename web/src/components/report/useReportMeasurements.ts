import { useEffect, useRef, useState } from "react";
import type {
  PageMeasurements,
  TableMeasurement,
} from "@/components/report/pagination";

export function useReportMeasurements({
  isPaged,
  layoutSignature,
}: {
  isPaged: boolean;
  layoutSignature: string;
}) {
  const measureRef = useRef<HTMLDivElement>(null);
  const [measurements, setMeasurements] = useState<PageMeasurements>({
    blockHeights: {},
    tableMeasurements: {},
    coverHeight: 0,
    tocHeight: 0,
    bibliographyHeaderHeight: 0,
    bibliographyItemHeights: [],
  });

  useEffect(() => {
    if (!isPaged) {
      return undefined;
    }
    let cancelled = false;

    const measure = () => {
      const container = measureRef.current;
      if (!container || layoutSignature.length === 0) {
        return;
      }

      const blockHeights: Record<string, number> = {};
      const tableMeasurements: Record<string, TableMeasurement> = {};

      const items =
        container.querySelectorAll<HTMLElement>("[data-measure-id]");
      for (const el of items) {
        const id = el.dataset.measureId;
        if (!id) continue;
        const blockHeight = el.getBoundingClientRect().height;
        blockHeights[id] = blockHeight;

        const theadEl = el.querySelector<HTMLElement>(
          '[data-measure-thead="true"]',
        );
        const tfootEl = el.querySelector<HTMLElement>(
          '[data-measure-tfoot="true"]',
        );
        const rowEls = el.querySelectorAll<HTMLElement>(
          "[data-measure-row-index]",
        );
        if (theadEl || rowEls.length > 0) {
          const rowHeights: number[] = [];
          for (const rowEl of rowEls) {
            rowHeights.push(rowEl.getBoundingClientRect().height);
          }
          const tfootHeight = tfootEl
            ? tfootEl.getBoundingClientRect().height
            : 0;
          const rowsHeight = rowHeights.reduce(
            (sum, height) => sum + height,
            0,
          );
          tableMeasurements[id] = {
            theadHeight: theadEl ? theadEl.getBoundingClientRect().height : 0,
            tfootHeight,
            rowHeights,
            chromeHeight: Math.max(0, blockHeight - rowsHeight - tfootHeight),
          };
        }
      }

      const coverEl = container.querySelector<HTMLElement>(
        '[data-measure-cover="true"]',
      );
      const tocEl = container.querySelector<HTMLElement>(
        '[data-measure-toc="true"]',
      );
      const bibEl = container.querySelector<HTMLElement>(
        '[data-measure-bib="true"]',
      );
      const bibRoot = bibEl?.querySelector<HTMLElement>(
        '[data-bibliography-root="true"]',
      );
      const bibHeader = bibRoot?.querySelector<HTMLElement>(
        '[data-bibliography-header="true"]',
      );
      let bibliographyHeaderHeight = 0;
      if (bibRoot && bibHeader) {
        const rootStyle = window.getComputedStyle(bibRoot);
        const headerStyle = window.getComputedStyle(bibHeader);
        bibliographyHeaderHeight =
          bibHeader.getBoundingClientRect().height +
          (Number.parseFloat(headerStyle.marginBottom) || 0) +
          (Number.parseFloat(rootStyle.marginTop) || 0) +
          (Number.parseFloat(rootStyle.paddingTop) || 0) +
          (Number.parseFloat(rootStyle.borderTopWidth) || 0);
      }
      const bibItems = bibRoot
        ? Array.from(bibRoot.querySelectorAll<HTMLElement>("[data-bib-index]"))
        : [];
      const bibliographyItemHeights = bibItems.map((item, index) => {
        const height = item.getBoundingClientRect().height;
        return height + (index + 1 < bibItems.length ? 14 : 0);
      });

      setMeasurements({
        blockHeights,
        tableMeasurements,
        coverHeight: coverEl ? coverEl.getBoundingClientRect().height : 0,
        tocHeight: tocEl ? tocEl.getBoundingClientRect().height : 0,
        bibliographyHeaderHeight,
        bibliographyItemHeights,
      });
    };

    void (async () => {
      await window.document.fonts.ready;
      await new Promise<void>((resolve) => {
        window.requestAnimationFrame(() => {
          window.requestAnimationFrame(() => resolve());
        });
      });
      if (!cancelled) {
        measure();
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [layoutSignature, isPaged]);

  return { measureRef, measurements };
}
