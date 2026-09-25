/**
 * Translation for the dashboard.
 *
 * No dependency and no network: the dictionaries are bundled with the
 * application like everything else (spec §130), and switching language is a
 * local settings change, not a download.
 */
import { createContext, useContext, useMemo, type ReactNode } from 'react';

import {
  figures,
  setDateLocale,
  setDurationUnits,
  setPersianFigures,
  setTwentyFourHour,
} from '../services/digits';
import type { Language } from '../types';
import { en } from './en';
import { es } from './es';
import { ca } from './ca';
import { fa } from './fa';

export type MessageKey = keyof typeof en;
/**
 * Keys are checked against English, values are just strings: a typo in a key
 * fails the build, a translation of a value never has to match it.
 */
export type Dictionary = Record<MessageKey, string>;
/** A real language, once 'system' has been resolved to one. */
export type Locale = Exclude<Language, 'system'>;

const DICTIONARIES: Record<Locale, Partial<Dictionary>> = { en, es, ca, fa };

/** Languages written right to left. */
const RTL: ReadonlySet<string> = new Set(['fa']);

export const LOCALES: Locale[] = ['en', 'es', 'ca', 'fa'];

/**
 * Which language to actually show.
 *
 * 'system' follows the desktop: someone who has already set their machine to
 * Catalan should not have to say it a second time. An unknown system language
 * falls back to English rather than to nothing.
 */
export function resolveLocale(language: Language | undefined): Locale {
  if (language && language !== 'system') return language;
  const tags =
    typeof navigator !== 'undefined'
      ? (navigator.languages ?? [navigator.language]).filter(Boolean)
      : [];
  for (const tag of tags) {
    const base = tag.toLowerCase().split('-')[0] as Locale;
    if (base && base in DICTIONARIES) return base;
  }
  return 'en';
}

export function directionOf(locale: Locale): 'rtl' | 'ltr' {
  return RTL.has(locale) ? 'rtl' : 'ltr';
}

/** Values are substituted into `{name}` placeholders. */
export type Vars = Record<string, string | number>;

function fill(template: string, vars?: Vars): string {
  if (!vars) return template;
  // Counts arrive as raw numbers; durations and times arrive already
  // formatted. Both go through the script's own figures.
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? figures(vars[name] ?? '') : whole,
  );
}

export interface Translation {
  locale: Locale;
  dir: 'rtl' | 'ltr';
  t: (key: MessageKey, vars?: Vars) => string;
}

const FALLBACK: Translation = {
  locale: 'en',
  dir: 'ltr',
  t: (key, vars) => fill(en[key], vars),
};

/**
 * A translator without React.
 *
 * The floating timer bar is its own framework-free page — it cannot reach a
 * context — and the same resolution rules have to apply there, so they live
 * here rather than inside the provider.
 */
/**
 * The translator the modules outside React use.
 *
 * The API layer turns a failed command into a message a person reads, and it
 * is not a component, so it cannot reach the context. Kept in step by
 * `makeTranslation`, the same way the figures registry is.
 */
let current: Translation | null = null;

/** For code that has no component around it. Falls back to English. */
export function translate(key: MessageKey, vars?: Vars): string {
  return current ? current.t(key, vars) : fill(en[key], vars);
}

export function makeTranslation(language: Language | undefined): Translation {
  const locale = resolveLocale(language);
  const dictionary = DICTIONARIES[locale];
  // Set before anything renders, so the first paint already has the right
  // figures rather than swapping them a frame later.
  setPersianFigures(locale === 'fa');
  setDurationUnits({
    hour: dictionary['unit.hour'] ?? en['unit.hour'],
    minute: dictionary['unit.minute'] ?? en['unit.minute'],
    second: dictionary['unit.second'] ?? en['unit.second'],
  });
  // Iran reads a 24-hour clock; an AM/PM suffix is dropped, not translated.
  setTwentyFourHour(locale === 'fa');
  // And the Persian calendar, so a date reads as the one someone would write.
  setDateLocale(locale === 'fa' ? 'fa-IR-u-ca-persian' : undefined);

  const translation: Translation = {
    locale,
    dir: directionOf(locale),
    // A key the translation has not reached yet reads in English rather
    // than showing its own name: a half-finished locale stays usable.
    t: (key, vars) => fill(dictionary[key] ?? en[key], vars),
  };
  current = translation;
  return translation;
}

const I18nContext = createContext<Translation>(FALLBACK);

export function I18nProvider({
  language,
  children,
}: {
  language: Language | undefined;
  children: ReactNode;
}) {
  const value = useMemo<Translation>(() => makeTranslation(language), [language]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useTranslation(): Translation {
  return useContext(I18nContext);
}
