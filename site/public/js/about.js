// Página Sobre: troca de idioma (escolha salva no navegador, senão o idioma do navegador) e o
// rating de blitz atual do bot.
import { pickLanguage } from "./lib/i18n.js";

const KEY = "caipora-lang";

function saved() {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

function apply(lang) {
  document.documentElement.lang = lang === "pt" ? "pt-BR" : "en";
  for (const block of document.querySelectorAll("[data-lang-block]")) block.hidden = block.dataset.langBlock !== lang;
  for (const button of document.querySelectorAll("[data-lang]")) button.setAttribute("aria-pressed", String(button.dataset.lang === lang));
  for (const node of document.querySelectorAll("[data-pt][data-en]")) node.textContent = node.dataset[lang];
  document.title = lang === "pt" ? "Sobre o Caipora" : "About Caipora";
}

for (const button of document.querySelectorAll("[data-lang]")) {
  button.addEventListener("click", () => {
    try {
      localStorage.setItem(KEY, button.dataset.lang);
    } catch {
      // sem armazenamento, a escolha vale só nesta visita
    }
    apply(button.dataset.lang);
  });
}

apply(pickLanguage(saved(), navigator.languages));

fetch("https://lichess.org/api/user/caiporaBot", { headers: { Accept: "application/json" } })
  .then((r) => r.json())
  .then((user) => {
    const rating = user.perfs?.blitz?.rating;
    if (rating) for (const node of document.querySelectorAll('[data-live="blitz"]')) node.textContent = String(rating);
  })
  .catch(() => {});
