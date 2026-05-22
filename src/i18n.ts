import en from "../i18n/en.json";
import es from "../i18n/es.json";

const tables: Record<string, any> = { en, es };

export function t(lang: string, key: string): string {
  const table = tables[lang] ?? tables.en;
  return key.split(".").reduce<any>((acc, seg) => (acc ? acc[seg] : undefined), table) ?? key;
}
