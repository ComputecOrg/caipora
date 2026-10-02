// Qual posição o motor analisa a seguir: a mais recente fundo; depois as anteriores mais rasas,
// da mais nova para a mais velha (é onde quem assiste está olhando).
export const LIVE_DEPTH = 22;
export const FILL_DEPTH = 16;

export function nextJob(count, evals) {
  if (count === 0) return null;
  const latest = count - 1;
  if ((evals[latest]?.depth ?? 0) < LIVE_DEPTH) {
    return { index: latest, depth: LIVE_DEPTH };
  }
  for (let i = latest - 1; i >= 0; i--) {
    if ((evals[i]?.depth ?? 0) < FILL_DEPTH) {
      return { index: i, depth: FILL_DEPTH };
    }
  }
  return null;
}
