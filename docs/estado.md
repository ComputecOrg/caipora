# Estado do Caipora

## v2 — 27/09/2026 (PR #7, busca da fase 2)

- **Força estimada: ~2440 CCRL Blitz** (+450 a +550 sobre a v1). Gauntlet a 8+0.08
  (`scripts/wsl_gauntlet.sh`, 60 partidas por adversário, commit 723a602):

  | Adversário (CCRL Blitz) | Placar do Caipora | Diferença |
  |---|---|---|
  | Stash 17 (2297) | 66,7% | +120 |
  | Stash 19 (2473) | 48,3% | −12 |
  | Stash 21 (2713) | 20,0% | −240 |
  | Stash 25 (2933) | 3,3% | −585 (placar extremo, pouco informativo) |

  Performance sobre as 240 partidas: ~2440. Nenhuma derrota do Caipora por tempo; as 3 perdas por
  tempo do gauntlet foram do Stash (v17 duas, v19 uma).
- **O que entrou:** RFP, null move, history com gravity, killers, LMR, IIR, "improving", LMP,
  futility, SEE (poda, ordenação e busca quiescente). SPRTs de nós fixos (100 mil nós por lance,
  [0, 10]), cada grupo contra o anterior:

  | Grupo | Resultado | Partidas |
  |---|---|---|
  | A: RFP + NMP | +82 ± 29 | 418 |
  | B: history + killers + LMR | +233 ± 48 | 198 |
  | C: IIR + improving + LMP + futility | +204 ± 45 | 210 |
  | D: SEE | ~+153 (70,8%) | 248 |

  Nós fixos inflam o ganho de quem economiza nós; a régua de verdade é o gauntlet acima.
- **Bench:** 3203074 nós (profundidade 12, que passou a ser o padrão; a profundidade 7 ficou rasa
  demais depois das podas).
- **Online:** o bot **caiporaBot** roda a v2 desde 27/09/2026 01:36, ainda casual, agora com
  matchmaking (desafia outros bots em 3+2 e 5+3).

## v1 — 26/09/2026 (PRs #1 a #4)

- **Força estimada: ~1900–2000 CCRL Blitz.** 40,8% contra o Stash 12 (1883) e 26,7% contra o
  Stash 15.3 (2173).
- **Bench:** 19604408 nós (profundidade 7 na época).
- **Robustez:** `fastchess --compliance` 40/40; 200 partidas de self-play e 240 do gauntlet sem
  nenhuma derrota por tempo, lance ilegal ou travamento.
- **Online desde 26/09/2026.** Primeira partida: <https://lichess.org/P8P6tjJU>, empate contra o
  sseh-c (1962 blitz) em 3+2, com o relógio apertado (7 s contra 107 s no lance 92).

## Fila de melhorias

Mudança de busca entra com SPRT de nós fixos (100 mil nós, livro 8moves_v3, [0, 10]); mudança de
tempo, com SPRT de relógio (8+0.08). Branches da fase 3 são empilhados (D10).

1. **Gestão de tempo** — PR #8. Não começar nova iteração depois de 60% do limite suave. Primeira
   rodada (665 partidas) parou em +8 Elo, inconclusiva; SPRT de relógio em andamento (D9).
2. **Busca, fase 3:**
   - correction history pela estrutura de peões — PR #9, SPRT em andamento;
   - continuation history (1 e 2 lances atrás) — PR #10, empilhado no #9;
   - a seguir: capture history, TT na busca quiescente, history no LMR, extensões singulares.
3. **Primeira NNUE** (D2, D11):
   - gerador de dados `caipora datagen` — PR #11 (~190 posições/s por núcleo);
   - treino no `bullet` com a GTX 1660 (CUDA no WSL);
   - inferência na engine.

   O Texel tuning da avaliação à mão saiu da fila: a NNUE vai substituí-la (D11).
