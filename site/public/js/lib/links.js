// Link para o perfil de um jogador no Lichess.
export function profileUrl(name) {
  return name ? `https://lichess.org/@/${encodeURIComponent(name)}` : null;
}
