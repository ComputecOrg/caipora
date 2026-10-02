# Estado do Caipora

## v3.7 — 02/10/2026 (noite de busca e velocidade, régua [0, 5])

- **Força estimada: ~3572 ± 24 CCRL Blitz** com 1 thread (+~64 sobre a v3.6). Gauntlet na AWS a
  8+0.08, 300 partidas por adversário, todas com terminação normal
  (`tools/aws-sprt/gauntlet-pack1/`): Stash 33 85,5% (=> 3581), Stash 37 70,2% (=> 3568). Pela lista
  CCRL de 26/09/2026, região do 78º lugar.
- **SPRT do pacote contra a v3.6** (STC 8+0.08, [0, 5]): **+48,6 ± 11,8** em 882 partidas (H1).
- **O que entrou** (cada um contra a v3.6, STC 8+0.08, [0, 5], D25; todos com terminação normal):
  - cut node e LMR mais rica (capturas e xeques também reduzem, menos; mais em cut node, menos
    melhorando; re-busca um nível mais funda ou mais rasa): **+11,1 ± 5,6** em 3613 (#68);
  - histórico de quietos por balde de ameaça, continuação de 4 meios-lances e bônus ao lance do
    pai quando o nó falha baixo: **+20,2 ± 7,9** em 1932 (#69);
  - cache de acumuladores por bucket do rei e atualização numa passada só, sem mudar a avaliação
    (bench idêntico), ~+12% de nós/s: **+21,9 ± 8,0** em 1679 (#70);
  - histórico de capturas com ameaça na casa de destino, na ordem e na fronteira de SEE entre
    capturas boas e ruins: **+6,1 ± 3,9** em 7119 (#71; reprovada em [0, 10] na versão antiga).
- **Reprovadas ou sem ganho:**
  - escala da avaliação pelo material: −5,4 ± 5,2 depois de 5000 partidas (parada perto do H0);
  - segundo estágio da g6 com wdl 0,3: −4,2 ± 5,5 contra a g6 (a variante wdl 0,6 nem treinou).
- **No bot desde 02/10/2026, ~03h40** (`caipora-89360b4`); config anterior em
  `config.yml.bak-a854a73`.
- **Bench:** 6011459.

## v3.6 — 30/09/2026 (rede g6 com buckets, extensões singulares, tempo por estabilidade)

- **Força estimada: ~3508 ± 23 CCRL Blitz** com 1 thread (+~54 sobre a v3.5), **acima do Tucano
  (3491)**. Gauntlet na AWS em 01/10/2026 a 8+0.08, 300 partidas por adversário, todas com
  terminação normal (`tools/aws-sprt/gauntlet-v3.6/`): Stash 33 80,2% (=> 3516), Stash 37 62,0%
  (=> 3504).
- **Primeira release pública:** 3.6 (`Cargo.toml` passou de 0.1.0 a 3.6.0), binários Windows e
  Linux, AVX2 e x86-64 genérico.
- **O que entrou** (todos com terminação normal; logs em `tools/aws-sprt/`):
  - rede g6 (D23): king buckets espelhados + output buckets, só dados do Lc0. SPRT de relógio
    contra a g5: **+22,9 ± 12,2** em 1036 partidas (local, à parte: +17,8 ± 14,2 em 800);
  - extensões singulares com multi-cut: SPRT de nós fixos **+28,1 ± 13,6** em 808 partidas;
  - limite suave do tempo pela estabilidade do melhor lance e pela queda da pontuação: SPRT de
    relógio **+16,2 ± 9,2** em 1288 partidas.
- **Reprovadas ou neutras:**
  - LMR guiada pelo histórico: +0,3 ± 6,6 em 3350 partidas;
  - pontuação da TT refinando a eval do null move e da futility: +1,5 ± 6,9 em 2982;
  - razoring, sem SPRT: perdia o mate em dois do teste, mesmo só até a profundidade 2.
- **No bot desde 30/09/2026, 23h20** (`caipora-a854a73`).
- **Bench:** 5108549. A g6 faz ~23% menos nós/s que a g5 (472 mil contra 611 mil no WSL).

## v3.5 — 30/09/2026 (rede g5, 1024 na camada oculta)

- **Força estimada: ~3454 ± 22 CCRL Blitz** com 1 thread. Gauntlet na AWS a 8+0.08, 300
  partidas por adversário, todas com terminação normal (`tools/aws-sprt/gauntlet-g5/`): Stash 33
  75,2%, Stash 37 (3419) 53,8%.
- **O que entrou:** rede g5 (D22), 1024 na oculta, só dados do Lc0 (mar, jun e ago/2023, ~81 GB).
  SPRT de relógio contra a g4: +41 ± 17 em 529 partidas.
- **Bench:** 2619291.
- **Tucano (3491):** a ~35 pontos.

## v3.4 — 28/09/2026 (PR da g4: rede treinada com dados do Lc0)

- **Força estimada: ~3435 ± 22 CCRL Blitz** com 1 thread (+~190 sobre a v3.3). Gauntlet na AWS
  a 8+0.08, 300 partidas por adversário, todas com terminação normal
  (`tools/aws-sprt/gauntlet-g4/`):

  | Adversário (CCRL Blitz) | v3.4 (g4) | v3.3 (g3) |
  |---|---|---|
  | Stash 33 (3273) | 73,2% | 44,2% |
  | Stash 37 (3419) | 50,8% | 26,8% |

- **O que entrou:** rede g4 (D20, D21), 512 na camada oculta, treinada só com dados do Lc0
  (ODbL), escala da engine 0,70 (`SCALE` 280). SPRT de relógio contra a g3: +237 ± 45.
- **No bot:** livro (chessdb + nuvem do Lichess) e tablebase online até 7 peças (D19).
- **Bench:** 2472196.
- **Tucano (3491):** a ~55 pontos.
- **Distância até o topo:** contra o Stockfish 19 oficial (lançado em 05/09/2026), os dois com 1
  thread, 8+0.08, 200 partidas na AWS: 0 vitórias, 30 empates, 170 derrotas (7,5%), −436 ± 62
  Elo (`tools/aws-sprt/sf19/`). Pela régua do gauntlet, o Stockfish estaria em ~3870.

## v3.3 — 28/09/2026 (PRs #47 e #50: ponder e rede g3 com 512)

- **Força estimada: ~3242 ± 23 CCRL Blitz** com 1 thread (+~135 sobre a v3.2). Gauntlet na AWS a
  8+0.08, Hash 16, 200 partidas por adversário, 600 partidas, todas com terminação normal
  (`tools/aws-sprt/gauntlet-g3-512/`):

  | Adversário (CCRL Blitz) | v3.3 (g3-512) | g3-256 | v3.2 (g2) |
  |---|---|---|---|
  | Stash 30 (3153) | 63,5% | 60,8% | 45,2% |
  | Stash 33 (3273) | 44,2% | 41,2% | 24,5% |
  | Stash 37 (3419) | 26,8% | 22,2% | 12,0% |

  A g2 repetiu 3094 ± 26 no gauntlet da g3-256 (3108 na véspera): régua estável.
- **O que entrou:**
  - rede g3 (D18): 429 milhões de posições jogadas pela g2, camada oculta de 512. SPRTs de
    relógio: g3-256 +162 ± 35 sobre a g2; g3-512 +30,5 ± 15,0 sobre a g3-256;
  - ponder (D17): +84 ± 18 contra sem ponder, só no bot (as listas jogam sem ponder).
- **Bench:** 2654286.

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
- **Bot:** 2 threads (a VPS tem 2 vCPUs), rated, adversários a até 300 pontos do rating dele,
  **com ponder** (D17): +84 ± 18 contra o Caipora sem ponder, pelo python-chess como no
  lichess-bot. O ponder não muda a força nas listas (CCRL e gauntlet jogam sem ponder).

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

1. **Cache de acumuladores por bucket do rei:** recuperar parte da velocidade perdida com a g6.
2. **Escala da g6:** reajustar `SCALE` com partidas (a g4 ganhou muito com isso, D21).
3. **g7:** mais blocos de treino; a perda de validação da g6 ainda caía na época 120.
4. **Tempo na abertura:** com o livro online no bot, medir de novo antes de mexer.
5. **Ferramentas:**
   - `caipora validate`: perda de validação;
   - `caipora-crash.log`: registro de quedas;
   - `scripts/wsl_train.sh`: treino com um comando.
