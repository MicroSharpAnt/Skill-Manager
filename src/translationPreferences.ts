import { useSyncExternalStore } from "react";
import type { TranslationStyle } from "./translation";
const key = "skill-manager-translation-style";
const eventName = "translation-style-changed";
export function readTranslationStyle(): TranslationStyle {
  const value = localStorage.getItem(key);
  return value === "translation" || value === "columns" ? value : "sentences";
}
export function saveTranslationStyle(value: TranslationStyle) {
  localStorage.setItem(key, value);
  window.dispatchEvent(new Event(eventName));
}
function subscribe(listener: () => void) {
  window.addEventListener(eventName, listener);
  window.addEventListener("storage", listener);
  return () => { window.removeEventListener(eventName, listener); window.removeEventListener("storage", listener); };
}
export function useTranslationStyle() {
  return useSyncExternalStore(subscribe, readTranslationStyle, () => "sentences" as const);
}
