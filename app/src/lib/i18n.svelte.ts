// Translations from the repository's lang/*.json, shared with the Rust side (Explorer menu).

const files = import.meta.glob<Record<string, string>>("../../../lang/*.json", {
  eager: true,
  import: "default",
});

const translations: Record<string, Record<string, string>> = {};
for (const [path, map] of Object.entries(files)) {
  const code = path.split("/").pop()!.replace(".json", "");
  translations[code] = map;
}

export const languages = Object.keys(translations).sort();

const lang = $state({ code: "en" });

export function setLanguage(code: string) {
  lang.code = translations[code] ? code : "en";
  document.documentElement.lang = lang.code;
}

/** Translated text; "{}" placeholders are filled in order, like Python's str.format. */
export function t(key: string, ...args: (string | number)[]): string {
  let text = translations[lang.code]?.[key] ?? translations.en?.[key] ?? key;
  for (const arg of args) text = text.replace("{}", String(arg));
  return text;
}
