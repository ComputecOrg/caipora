// Página Sobre: troca de idioma (escolha salva no navegador, senão o idioma do navegador) e o
// rating de blitz atual do bot.
import { pickLanguage } from "./lib/i18n.js";
import { activeSection } from "./lib/sections.js";

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
    markSection();
  });
}

apply(pickLanguage(saved(), navigator.languages));

// Barra fixa: destaca a seção em que o leitor está e a mantém visível na barra (celular).
function markSection() {
  const nav = document.querySelector(".sections:not([hidden])");
  if (!nav) return;
  const links = [...nav.querySelectorAll("a")];
  const targets = links.map((a) => document.getElementById(a.hash.slice(1)));
  const tops = targets.map((t) => t.getBoundingClientRect().top + scrollY);
  const atBottom = innerHeight + scrollY >= document.documentElement.scrollHeight - 2;
  const active = activeSection(tops, scrollY, nav.offsetHeight + 48, atBottom);
  links.forEach((a, i) => {
    if (i !== active) return a.removeAttribute("aria-current");
    if (a.getAttribute("aria-current") === "true") return;
    a.setAttribute("aria-current", "true");
    nav.scrollTo({ left: a.offsetLeft - (nav.clientWidth - a.offsetWidth) / 2, behavior: "smooth" });
  });
}

addEventListener("scroll", markSection, { passive: true });
addEventListener("resize", markSection);
markSection();

fetch("https://lichess.org/api/user/caiporaBot", { headers: { Accept: "application/json" } })
  .then((r) => r.json())
  .then((user) => {
    const rating = user.perfs?.blitz?.rating;
    if (rating) for (const node of document.querySelectorAll('[data-live="blitz"]')) node.textContent = String(rating);
  })
  .catch(() => {});
