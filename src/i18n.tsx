import { createContext, useContext, type ReactNode } from "react";
import { en } from "./locales/en";
import { zh } from "./locales/zh";

export type Lang = "zh" | "en";
export type MessageKey = keyof typeof zh;
export type Translate = (key: MessageKey, vars?: Record<string, string | number>) => string;

const DICTIONARIES: Record<Lang, Record<MessageKey, string>> = { zh, en };

export function translator(lang: Lang): Translate {
  const dict = DICTIONARIES[lang];
  return (key, vars) =>
    dict[key].replace(/\{(\w+)\}/g, (match, name: string) =>
      vars && name in vars ? String(vars[name]) : match,
    );
}

/** Used until the backend reports the resolved language. */
export function browserLang(): Lang {
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

type I18n = { lang: Lang; t: Translate };

const I18nContext = createContext<I18n>({ lang: "zh", t: translator("zh") });

export function I18nProvider({ lang, children }: { lang: Lang; children: ReactNode }) {
  return (
    <I18nContext.Provider value={{ lang, t: translator(lang) }}>{children}</I18nContext.Provider>
  );
}

export const useI18n = () => useContext(I18nContext);

/** Renders `code` spans written with backticks in a translated string. */
export function withCode(text: string) {
  return text
    .split("`")
    .map((part, i) => (i % 2 === 1 ? <code key={i}>{part}</code> : part));
}
