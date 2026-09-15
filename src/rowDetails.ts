import type { MouseEvent } from "react";

// Keep nested controls independent; the existing name button remains the keyboard entry.
export function openRowDetails(event: MouseEvent<HTMLElement>, open: () => void) {
  if (event.defaultPrevented || !(event.target instanceof Element)) return;
  if (event.target.closest("button, input, a, label, select, textarea, [role='button'], [role='checkbox'], [role='switch']")) return;
  const selection = window.getSelection();
  if (selection && !selection.isCollapsed && event.currentTarget.contains(selection.anchorNode)) return;
  open();
}
