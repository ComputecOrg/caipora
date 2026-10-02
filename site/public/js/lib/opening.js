// Nome da abertura do Lichess ("Família: variação, subvariação") → abertura e defesa/variação.
export function splitOpening(data) {
  if (!data?.name) return null;
  const i = data.name.indexOf(":");
  if (i < 0) return { eco: data.eco ?? null, opening: data.name, variation: null };
  return { eco: data.eco ?? null, opening: data.name.slice(0, i).trim(), variation: data.name.slice(i + 1).trim() };
}
