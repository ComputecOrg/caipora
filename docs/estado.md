# Estado do Caipora

## v3.2 — 27/09/2026 (PRs #38, #39 e #45: Lazy SMP e rede g2)

- **Força estimada: ~3108 ± 24 CCRL Blitz** com 1 thread (+~230 sobre a v3.1). Gauntlet na AWS
  a 8+0.08, Hash 16, 200 partidas por adversário. A v3.1 (rede g1) jogou o mesmo gauntlet como
  régua e saiu em 2850 ± 30, dentro do erro dos ~2879 medidos em casa:

  | Adversário (CCRL Blitz) | v3.2 (g2) | v3.1 (g1), mesmo gauntlet |
  |---|---|---|
  | Stash 25 (2933) | 76,5% | 40,5% |
  | Stash 30 (3153) | 42,2% | 15,5% |
  | Stash 33 (3273) | 26,0% | 5,0% |

  1400 partidas, todas com terminação normal (`tools/aws-sprt/gauntlet-g2/`).
- **O que entrou:**
  - rede g2 (D16), 159 milhões de posições jogadas pela g1: +338 ± 53 em nós fixos e
    +331 ± 57 a 8+0.08, contra a g1;
  - Lazy SMP (opção `Threads`, até 256): 2 threads contra 1, +121 ± 31 a 8+0.08 em 275
    partidas;
  - TT sem trava (entradas atômicas), pré-requisito do Lazy SMP.
- **Bench:** 2893409.
- **Bot:** 2 threads (a VPS tem 2 vCPUs), rated, contra bots de 2000 a 2600.

## v3.1 — 27/09/2026 (PRs #10, #15, #16: busca da fase 3 com a rede)

- **Força estimada: ~2879 CCRL Blitz** (+64 sobre a v3). Gauntlet a 8+0.08, 80 partidas por
  adversário, main 62d57c0:

  | Adversário (CCRL Blitz) | Placar do Caipora | Na v3 |
  |---|---|---|
  | Stash 19 (2473) | 89,4% | 86,9% |
  | Stash 21 (2713) | 71,3% | 65,6% |
  | Stash 25 (2933) | 45,0% | 33,8% |

  Nenhuma derrota do Caipora por tempo; as 2 perdas por tempo foram do Stash 21.
- **O que entrou.** SPRTs de nós fixos na AWS (100 mil nós, rede nos dois lados):
  - continuation history: +28 ± 15, aprovada;
  - TT em grupos de 4: +5,6 ± 6,3 em 5.658 partidas, pela regra D15;
  - TT na busca quiescente: +50 ± 20, aprovada.
- **Não entrou:**
  - capture history: −13 ± 12, reprovada;
  - LMR por histórico: medida contra a capture history, fica para reteste.
- **Bench:** 3306025. Roda no bot do servidor desde 27/09/2026 ~14:15.
- **Rodada de dados g2 perdida.** Os ~138 milhões de posições se perderam por falha na coleta. A
  coleta foi corrigida (PR #34) e a rodada refeita deu a rede da v3.2.

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

1. **Ponder** (pensar no tempo do adversário): o bot joga uma partida por vez e deixa os 2
   núcleos parados enquanto o adversário pensa; a engine ainda ignora `go ponder`.
2. **Terceira geração de dados (g3)**, jogada pela g2; com mais dados, testar uma camada oculta
   maior que 256.
3. **LMR guiada pelo histórico** (branch `feat/history-lmr`): retestar em cima da main.
4. **4 threads no bot:** medir 4 contra 2 na AWS antes de pensar em VPS maior (nos EUA a
   Hetzner ficou cara em jun/2026; na Europa um CPX32 cabe no orçamento e fica perto do Lichess).
5. **Ferramentas:**
   - `caipora validate`: perda de validação;
   - `caipora-crash.log`: registro de quedas;
   - `scripts/wsl_train.sh`: treino com um comando.
