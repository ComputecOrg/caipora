// Idioma da página Sobre: a escolha salva vale primeiro; depois o idioma do navegador; senão inglês.
export const LANGUAGES = ["pt", "en"];

export function pickLanguage(saved, browserLanguages) {
  if (LANGUAGES.includes(saved)) return saved;
  for (const tag of browserLanguages || []) {
    const base = String(tag).toLowerCase().split("-")[0];
    if (LANGUAGES.includes(base)) return base;
  }
  return "en";
}
