import type { TFunction } from 'i18next';

export type LanguageOption = {
  value: string;
  label: string;
};

function normalizeLanguageCode(code: string | null | undefined): string {
  switch (code?.trim().toLowerCase()) {
    case 'zh-hans':
    case 'zh-cn':
      return 'zh';
    case 'zh-hant':
    case 'zh-tw':
    case 'zh-hk':
      return 'zh-tw';
    case 'ja-jp':
      return 'ja';
    case 'ko-kr':
      return 'ko';
    case 'en-us':
    case 'en-gb':
    case 'en-au':
    case 'en-ca':
      return 'en';
    case 'es-es':
    case 'es-mx':
    case 'es-ar':
      return 'es';
    case 'fr-fr':
    case 'fr-ca':
      return 'fr';
    case 'de-de':
    case 'de-at':
      return 'de';
    case 'it-it':
      return 'it';
    case 'pt-pt':
    case 'pt-br':
      return 'pt';
    case 'ru-ru':
      return 'ru';
    case 'ar-sa':
    case 'ar-ae':
      return 'ar';
    case 'th-th':
      return 'th';
    case 'vi-vn':
      return 'vi';
    case 'nl-nl':
    case 'nl-be':
      return 'nl';
    case 'pl-pl':
      return 'pl';
    case 'tr-tr':
      return 'tr';
    case 'id-id':
      return 'id';
    case 'hi-in':
      return 'hi';
    default:
      return code?.trim().toLowerCase() || '';
  }
}

export function getLanguageOptions(t: TFunction<'translation'>): LanguageOption[] {
  return [
    { value: 'en', label: t('ocr.languages.english') },
    { value: 'zh', label: t('ocr.languages.chineseSimplified') },
    { value: 'zh-tw', label: t('ocr.languages.chineseTraditional') },
    { value: 'ja', label: t('ocr.languages.japanese') },
    { value: 'ko', label: t('ocr.languages.korean') },
    { value: 'es', label: t('ocr.languages.spanish') },
    { value: 'fr', label: t('ocr.languages.french') },
    { value: 'de', label: t('ocr.languages.german') },
    { value: 'it', label: t('ocr.languages.italian') },
    { value: 'pt', label: t('ocr.languages.portuguese') },
    { value: 'ru', label: t('ocr.languages.russian') },
    { value: 'ar', label: t('ocr.languages.arabic') },
    { value: 'th', label: t('ocr.languages.thai') },
    { value: 'vi', label: t('ocr.languages.vietnamese') },
    { value: 'nl', label: t('ocr.languages.dutch') },
    { value: 'pl', label: t('ocr.languages.polish') },
    { value: 'tr', label: t('ocr.languages.turkish') },
    { value: 'id', label: t('ocr.languages.indonesian') },
    { value: 'hi', label: t('ocr.languages.hindi') },
  ];
}

export function getSourceLanguageOptions(t: TFunction<'translation'>): LanguageOption[] {
  return [{ value: 'auto', label: t('widget.auto') }, ...getLanguageOptions(t)];
}

export function prioritizeLanguageOption(
  options: LanguageOption[],
  preferred: string | null | undefined,
  fallbackLabel: string,
): LanguageOption[] {
  const normalizedPreferred = normalizeLanguageCode(preferred);
  const matched = normalizedPreferred
    ? options.find((option) => option.value === normalizedPreferred)
    : undefined;

  const preferredOption = matched ?? {
    value: normalizedPreferred || options[0]?.value || 'auto',
    label: matched?.label ?? fallbackLabel,
  };

  const remaining = options.filter((option) => option.value !== preferredOption.value);
  return [preferredOption, ...remaining];
}

export function normalizeSelectableLanguage(code: string | null | undefined, fallback: string): string {
  const normalized = normalizeLanguageCode(code);
  return normalized || fallback;
}
