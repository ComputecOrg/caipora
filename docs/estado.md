# Estado do Caipora

## v1 — 26/09/2026 (PRs #1 a #4)

- **Força estimada: ~1900–2000 CCRL Blitz.** Gauntlet a 8+0.08 (`scripts/wsl_gauntlet.sh`,
  60 partidas por adversário): 40,8% contra o Stash 12 (1883) e 26,7% contra o Stash 15.3 (2173).
  Contra o Stash 19 e 21 o placar é desequilibrado demais para calibrar.
- **Bench:** 19604408 nós, idêntico no Windows e no Linux; ~3,4–3,5 milhões de nós/s no
  Ryzen 5 3600.
- **Robustez:** `fastchess --compliance` 40/40; 200 partidas de self-play (8+0.08 e 2+0.02) e 240
  do gauntlet sem nenhuma derrota do Caipora por tempo, lance ilegal ou travamento.
- **Online desde 26/09/2026:** conta **caiporaBot** (BOT) no Lichess, rodando no PC do dono
  (lichess-bot 2026.8.9.2, só partidas casual, sem matchmaking). Primeira partida:
  <https://lichess.org/P8P6tjJU>, empate de pretas contra o sseh-c (1962 blitz) em 3+2, 126 lances,
  sem erro nem perda por tempo. O relógio ficou apertado (7 s contra 107 s no lance 92), o que
  confirma o item 1 da fila.

## Fila de melhorias

Cada item entra num branch próprio, com SPRT (fastchess, 8+0.08, livro 8moves_v3, limites
[0, 10] enquanto a engine estiver abaixo de ~2500) e o resultado no PR.

1. **Gestão de tempo.** Com 10 s + 0,1 s a engine chega ao lance 30 com ~0,5 s no relógio: gasta
   cerca do dobro do limite suave, porque uma iteração iniciada perto dele vai até o limite duro.
   Candidato: não começar nova iteração depois de ~60% do limite suave.
2. **Busca, fase 2** (ordem da pesquisa, seção 5): RFP → NMP → history com gravity → killers →
   LMR → LMP → futility → IIR → SEE.
3. **Ajuste da avaliação** (Texel tuning) com posições de partidas do próprio Caipora (D8).
4. **Geração de dados e primeira NNUE** (D2): datagen no binário; treino no `bullet` com a
   GTX 1660 (CUDA).
