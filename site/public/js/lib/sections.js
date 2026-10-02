// Seção atual da página Sobre: a última cujo topo já passou da barra fixa. No fim da página a
// última seção vale mesmo curta demais para chegar à barra.
export function activeSection(tops, scrollY, barHeight, atBottom = false) {
  if (tops.length === 0) return -1;
  if (atBottom) return tops.length - 1;
  let active = 0;
  for (let i = 0; i < tops.length; i++) if (tops[i] <= scrollY + barHeight) active = i;
  return active;
}
