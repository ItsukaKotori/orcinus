export const UI_LANGUAGE_SYSTEM = 'system'
export const UI_LANGUAGE_ENGLISH = 'en'
export const UI_LANGUAGE_CHINESE = 'zh'
export const UI_LANGUAGE_KOREAN = 'ko'
export const UI_LANGUAGE_JAPANESE = 'ja'
export const UI_LANGUAGE_SPANISH = 'es'
export const UI_LANGUAGE_FRENCH = 'fr'

export type BuiltInUiLanguage =
  | typeof UI_LANGUAGE_SYSTEM
  | typeof UI_LANGUAGE_ENGLISH
  | typeof UI_LANGUAGE_CHINESE
  | typeof UI_LANGUAGE_KOREAN
  | typeof UI_LANGUAGE_JAPANESE
  | typeof UI_LANGUAGE_SPANISH
  | typeof UI_LANGUAGE_FRENCH

export type UiLanguage = BuiltInUiLanguage

const UI_LANGUAGE_VALUES = new Set<BuiltInUiLanguage>([
  UI_LANGUAGE_SYSTEM,
  UI_LANGUAGE_ENGLISH,
  UI_LANGUAGE_CHINESE,
  UI_LANGUAGE_KOREAN,
  UI_LANGUAGE_JAPANESE,
  UI_LANGUAGE_SPANISH,
  UI_LANGUAGE_FRENCH
])

export function normalizeUiLanguage(value: unknown): UiLanguage {
  return UI_LANGUAGE_VALUES.has(value as BuiltInUiLanguage)
    ? (value as BuiltInUiLanguage)
    : UI_LANGUAGE_SYSTEM
}
