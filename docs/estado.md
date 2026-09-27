# Estado do Caipora

## v3 — 27/09/2026 (PRs #24 e #27: primeira NNUE)

- **Força estimada: ~2815 CCRL Blitz** (+375 sobre a v2). Gauntlet a 8+0.08, 80 partidas por
  adversário, branch com a rede embutida (08c3562):

  | Adversário (CCRL Blitz) | Placar do Caipora | Diferença |
  |---|---|---|
  | Stash 19 (2473) | 86,9% | +328 |
  | Stash 21 (2713) | 65,6% | +112 |
  | Stash 25 (2933) | 33,8% | −117 |

  Nenhuma derrota do Caipora por tempo; as 2 perdas por tempo foram do Stash.
- **O que entrou:**
  - a rede `caipora-g1`, embutida no executável (D14): (768 → 256)×2 → 1, 10 milhões de
    posições de self-play próprio, alvo só na pontuação da busca (wdl 0);
  - correction history (+47 Elo em nós fixos);
  - a regra dos 60% do tempo (D13);
  - build x86-64-v3 (D12).
- **Bench:** 4214426 nós (com a rede), idêntico no Windows e no Linux (binário estático musl).
- **Online:** o bot **caiporaBot** roda desde 27/09/2026 ~10:00 na VPS da Hetzner do dono (EUA,
  88 ms até o Lichess), como serviço `caipora-bot` (`deploy/README.md`). Partidas casual, com
  matchmaking contra bots de 2000 a 2600. Primeiras partidas com a rede:
  - vitória contra o simpleEval (2103), com 97% de precisão na análise do Lichess;
  - derrota contra o botchessbot (2482), assumida pelo servidor com 5 s no relógio durante a
    troca de token.

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

- Mudança de busca: SPRT de nós fixos (100 mil nós, 8moves_v3, [0, 10]), **com a rede nos dois
  lados**.
- Mudança de tempo: SPRT de relógio a 8+0.08.
- Branches empilhados (D10).

1. **Busca, fase 3** (PRs em rascunho, na ordem):
   - continuation history (#10): com a rede, +26 ± 30 em 240 partidas antes da pausa;
   - TT em grupos de 4 (#15);
   - TT na busca quiescente (#16);
   - capture history (#20);
   - LMR guiada pelo histórico (#21).

   Os SPRTs anteriores, feitos com a avaliação à mão, deixaram de valer.
2. **Segunda geração de dados (g2)**, jogada e pontuada pela rede g1 (`caipora datagen` com
   rede, PR #26), para treinar a rede g2.
3. **Computação para as rajadas de testes e dados:** AWS spot (orçamento do dono: até
   ~R$ 100/mês). Pendente: login do AWS CLI, alerta de orçamento e aumento da cota de spot.
4. **Ferramentas:**
   - `caipora validate`: perda de validação;
   - `caipora-crash.log`: registro de quedas;
   - `scripts/wsl_train.sh`: treino com um comando.
