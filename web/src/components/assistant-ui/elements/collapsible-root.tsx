"use client";

import { Collapsible as CollapsiblePrimitive } from "@base-ui/react/collapsible";
import { forwardRef } from "react";

export const CollapsibleRoot = forwardRef<
  HTMLDivElement,
  CollapsiblePrimitive.Root.Props
>(function CollapsibleRoot(props, ref) {
  return (
    <CollapsiblePrimitive.Root ref={ref} data-slot="collapsible" {...props} />
  );
});

export function CollapsibleTrigger({
  ...props
}: CollapsiblePrimitive.Trigger.Props) {
  return (
    <CollapsiblePrimitive.Trigger
      data-slot="collapsible-trigger"
      {...props}
    />
  );
}

export function CollapsibleContent({
  ...props
}: CollapsiblePrimitive.Panel.Props) {
  return (
    <CollapsiblePrimitive.Panel
      data-slot="collapsible-content"
      {...props}
    />
  );
}
