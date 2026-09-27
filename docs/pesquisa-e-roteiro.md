# ChessAI — Pesquisa e roteiro (26/09/2026)

Consolidação de 10 frentes de pesquisa na web (8 temas técnicos + 2 de servidores e custos),
cada uma conferida por um verificador independente contra fontes primárias (listas CCRL/CEGT/SPCC ao vivo, código do
Lichess, do lichess-bot, do OpenBench, do Stockfish, PRs com SPRT, wiki do TCEC). Onde o
verificador corrigiu o pesquisador, vale a versão corrigida. Números de Elo sem rótulo são da
**CCRL Blitz**; os da **CCRL 40/15** vêm marcados.

---

## 0. Resumo executivo

1. **O topo está comprimido e alcançável — exceto o 1º lugar.** Na CCRL 40/15 (23/09/2026) as 20
   primeiras cabem em 25 Elo (Stockfish 19 = 3650, 20º = 3625). Em condições de torneio (aberturas
   desbalanceadas) o Stockfish fica ~77 Elo à frente do 2º (Reckless) e ~140 à frente do 10º.
   Ninguém vence uma Superfinal do TCEC contra o Stockfish desde 2020.
2. **Engines novas chegam ao top-10 em 1,5–3 anos** (Reckless, PlentyChess, pawnocchio,
   Obsidian). A **Coda**, escrita 100% pelo Claude Code, chegou a 3633 na CCRL 40/15 (#11) em
   ~8 meses — mas treinada com dados do Lc0.
3. **A receita é única e conhecida:** alpha-beta + NNUE em CPU, toda mudança aprovada por SPRT
   (fastchess/OpenBench), rede treinada no `bullet`, dados de self-play, hardware compartilhado
   pela comunidade. O caminho Lc0 (MCTS em GPU) não é viável para projeto novo.
4. **Decisão crítica antes do primeiro commit — uso de IA.** Desde 31/08/2026 o TCEC coloca
   engines "com grande ajuda de IA" na Categoria 4: só entram pelo Swiss, **não podem subir às
   ligas regulares** (vale para a Season 30, declarada "campo de teste"). A CCRL aceita, mas
   rotula "(AI assisted)". Ver §2.
5. **v1 jogando online em dias:** conta BOT no Lichess + `lichess-bot`. Há armadilhas concretas
   (rating inicial 3000, limite de 100 partidas bot×bot/dia, sem compensação de lag para bots,
   detector de farming). Ver §7.
6. **Sua máquina muda o plano:** Ryzen 5 3600 é Zen 2 → **PEXT é lento (microcódigo)**, usar
   magic bitboards; 6 núcleos físicos → ~730 partidas/h em STC, suficiente até a 1ª NNUE;
   GTX 1660 serve para as primeiras redes via CUDA (o `bullet` não roda mais em CPU).
7. **Dá para começar com R$ 0.** Servidor pago só vira necessidade por volta de ~3.300, quando
   cada teste passa a exigir dezenas de milhares de partidas; antes disso, GPU e spot por hora
   custam poucos reais por uso. Ver §8.
8. **Linguagem: Rust estável.** Reckless (#2 do mundo) é Rust; o treinador `bullet` é Rust;
   `cargo test` casa com TDD; a Coda (Claude Code) é Rust.
9. **Correção ≠ Elo.** SPRT mede força, não corretude: o Clockwork aprovou uma correction history
   com bug a +29,5 Elo. Invariantes (perft, hash incremental, mate na TT, simetria) vão em TDD;
   Elo vai em SPRT. As duas coisas, sempre.

---

## 1. Onde está o topo hoje

### 1.1 Rankings (set/2026)

| # | CCRL 40/15 (4 CPU, 23/09) | CCRL Blitz (1 CPU, 26/09) | SPCC UHO (24/09) |
|---|---|---|---|
| 1 | Stockfish 19 — 3650 | Stockfish 19 — 3793 | Stockfish dev — 3910 |
| 2 | Reckless 0.9.0 (Rust) — 3644 | Torch v4d — 3776 | Stockfish 19 — 3905 |
| 3 | PlentyChess 7.0 — 3643 | PlentyChess 8.0 — 3774 | Reckless — 3828 |
| 4 | pawnocchio 2.0.1 (Zig) — 3642 | Reckless 0.9.0 — 3766 | Torch 4d — 3811 |
| 5 | Torch v4d — 3639 | Cinder 0.6.1 (Rust) — 3763 | PlentyChess 8.0 — 3805 |
| 10 | Stormphrax 8.0 — 3634 | Obsidian 16.0 — 3747 | Obsidian — 3765 |
| 11–12 | Coda 0.9.4 (Rust, Claude Code) — 3633 | — | Coda (marcada "AI") — 3778 (#8) |

- Listas com livro balanceado têm 78–94% de empates → tudo comprime. Para medir progresso de
  verdade: livros UHO + resultado pentanomial.
- TCEC S29 (jan–abr/2026): Superfinal Stockfish 59–41 Reckless. S30 em andamento; Premier com
  Stockfish, Reckless, Lc0, PlentyChess, Torch, Integral (+2 vagas da League 1).
- Linguagens no top-25 da 40/15: 15 C++, 5 Rust, 2 C, 1 Zig, 2 fechadas. Linguagem vale
  ~15–26 Elo (port C#→C++ do Lizard/Horsie), irrelevante perto de rede e busca.

### 1.2 Trajetórias que servem de régua

| Engine | Início | Marco | Observação |
|---|---|---|---|
| Reckless (Rust) | mai/2023, ~2005 | 3765 (#2) em ago/2025; Superfinal TCEC em abr/2026 | virou equipe; OpenBench próprio; 384 threads de datagen |
| pawnocchio (Zig) | nov/2024 | 3120 em fev/2025 (ao ganhar NNUE); #4 na 40/15 em ~19 meses | dados "do zero" + jogos do Vine |
| Coda (Rust, Claude Code) | jan/2026 | 3633 na 40/15 em ~8 meses | dados Lc0 (ODbL); 2.700+ commits com SPRT |
| Clockwork (comunidade, HCE) | jun/2025 | ~3403 na 40/15 em ~1 ano | autores experientes; Elo medido por feature |
| Viridithas (Rust) | 2022 | 2477 → 2954 com 1ª NNUE → 3748 em 2026 | 100% dados próprios |
| Leorik (C#, solo) | 2022 | 2112 → 2537 (podas) → 3265 (1ª NNUE) → 3490 | dados próprios, desktop de 12 núcleos |

**Marcos de força observados (CCRL Blitz):**

| Estágio | Faixa típica |
|---|---|
| (a) alpha-beta + PST (+qsearch, MVV-LVA) | 1.300–1.800 (implementação muito rápida: ~2.100) |
| (b) + TT + ordenação + qsearch completo | 1.700–2.200 |
| (c) + podas modernas + avaliação tunada | 2.400–3.000 |
| (d) + 1ª NNUE simples com dados próprios | +150 a +480 → ~2.950–3.300 |
| (e) NNUE forte + anos de SPRT/SPSA + bilhões de posições | 3.500–3.750 |

Benchmark de set/2026 (Steve Maughan, **não é rating CCRL oficial**, 10+0.1 num laptop,
âncoras da CCRL Blitz): modelos Claude escrevendo sozinhos por 24 h chegaram a 2.702 (Sonnet 5)
… 3.463 (Opus 5.5). O Opus 5.5 tinha ~2.950 aos 16 min (PeSTO + busca completa) e a 1ª NNUE
(5,3M posições) deu +158. Dois bugs quase invisíveis custaram caro: chave de TT de 16 bits
(fix = +300) e derrotas por tempo a 2+0.02 (9 de 20 jogos) que só apareceram em stress test.

---

## 2. Decisões de dia 1: IA, originalidade, licença, dados

### 2.1 Política de IA do TCEC (o ponto mais importante desta pesquisa)

- **Questionário v2.4** (obrigatório para entrar): a pergunta 5 é "Does your engine contain AI
  generated code, if so which part(s)...". Respostas ficam públicas.
- **Categorias da S30** (definem qual playoff a engine disputa para chegar ao Swiss):
  - HCE (avaliação manual) → Cat 1.
  - NNUE com dados ORIGINAIS + inovações → Cat 1; sem inovações → Cat 2.
  - NNUE com dados de terceiros (Lc0/SF) sem inovações → Cat 3 (ex.: Cinder, 6ª da CCRL).
  - **Cat 4 = "created with major help from AI technology"**: só pelo playoff Cat 3+4 (1 vaga),
    código aberto compilado pelo TCEC, **inelegível para as ligas regulares** "qualquer que seja
    o resultado"; haverá um gauntlet separado.
- A fronteira "assistência menor" × "grande ajuda" **não está escrita**. Halogen (Copilot/ChatGPT
  "minor") segue nas ligas; CatGPT (quase tudo via Cursor) foi Cat 4. A regra é "for Season 30"
  e o comitê fala em futuras competições "separate and/or joint".
- **CCRL:** aceita, pede a declaração "coded on your own or AI assisted" e já rotula
  "(AI assistance)". SPCC marca "AI". Parte da comunidade é hostil (o README do Stormphrax diz
  "no LLM-generated code" e traz um parágrafo destrutivo dirigido a LLMs).

**Consequência:** o modo como vamos usar o Claude Code decide se o teto é "ligas do TCEC" ou
"listas + Swiss/gauntlet de IA". Opções na §11.

### 2.2 Originalidade e licença

- Casos que mancharam reputações para sempre: Rybka (ICGA, 2011, banimento vitalício), Houdini e
  Fire (removidos do TCEC por código copiado), Fat Fritz 2 (acordo judicial com o Stockfish por
  violar a GPL).
- **Ideias são livres; código e constantes não.** A comunidade aceita portar ideias do Stockfish
  entre linguagens (defesa do Reckless no TalkChess, mar/2026); copiar código obriga a adotar a
  licença (GPL-3.0 do Stockfish, **AGPL-3.0 da Viridithas desde jul/2026**).
- A Coda (Claude Code) descobriu código AGPL gerado pela IA na própria base e teve de fazer
  auditoria de licenças. Regra para nós: o agente **nunca porta código** de engine de terceiros;
  lê para entender a ideia e reimplementa.
- Licença recomendada: **GPL-3.0** (padrão do topo). Bibliotecas: preferir MIT (Fathom/Pyrrhic
  para Syzygy, `bullet`, `viriformat`); `shakmaty` é GPL.

### 2.3 Dados de treino da NNUE

- Diretriz (não oficial, mas perguntada no questionário) do TCEC: "All NNUE training data should
  be generated by the unique engine's own search and/or eval code."
- Dados do Lc0 são ODbL (legais, usados por Obsidian, Alexandria, Coda, Stockfish), mas contam
  como "OTHER" no TCEC e a comunidade chama o bootstrap via Lc0 de "laundering".
- **Recomendação: 100% dados próprios desde o início** (Stormphrax, Viridithas, PlentyChess desde
  a v3, Leorik). Trocar de política depois obriga a regerar tudo.

---

## 3. Onde competir — a escada

| Degrau | Requisito | Como entrar | Força típica |
|---|---|---|---|
| **Lichess BOT** | UCI estável | conta nova + token `bot:play` + `lichess-bot -u` | qualquer |
| **CCRL** (Blitz, 40/15, FRC) | .exe Windows AVX2, controle de tempo correto, livro próprio desativável, sai limpo | tópico próprio no fórum da CCRL ou TalkChess com nome/versão, força estimada, nacionalidade, código aberto?, declaração de IA | sem mínimo (lista vai até ~200) |
| **CEGT** 40/20 | idem | anúncio no TalkChess; testers escolhem | na prática >2.400 |
| **Amateur Series** (Graham Banks) | idem | via CCRL; estreantes alocados pela força | Div 14 ≈ 2.500–2.760 |
| **Chess Engines Diary** | ≥50% contra Critter 1.6a (≈3.228) a 3'+3" | teste preliminar | ~3.200+ |
| **TCEC** | Linux (Ubuntu 22.04), **FRC/DFRC nativo** (ligas jogam FRC), estável em muitos threads, sem livro interno | questionário v2.4 por e-mail; categorização; playoff → Swiss → Entrance League → L2 → L1 → Premier | playoffs a partir de ~3.050 observados; Swiss competitivo ~3.300+ |
| **SPCC** UHO-Top15 | estar entre as 15 melhores | — | ~3.730+ |
| **CCC** (Chess.com) | convite (critério de 2018: 3000+, SMP ≥8 threads, 100% estável) | convite | elite |

- WCCC/ICGA acabou em 2024. Chess.com proíbe bots. FICS aceita conta de computador (pedido por
  e-mail, exige conta humana prévia).
- TCEC: janela da S30 fechou em 01/07/2026; a da S31 não foi anunciada (estimativa: meados de
  2027). O servidor compila com `rustc 1.85.1` via `update.sh` — manter MSRV compatível ou
  entregar binário Linux estático. Hardware: 2× EPYC 9754 (512 threads), hash até 256 GiB.
- Porta lateral: as competições 4k/ELF deram vaga de Cat 1 a engines HCE minúsculas.

---

## 4. Arquitetura base (fazer certo desde o início)

| Tema | Decisão | Por quê |
|---|---|---|
| Linguagem | Rust estável, edition 2024, toolchain fixado (`rust-toolchain.toml`) | a Superfinal S29 foi reiniciada porque um Rust novo deixou o Reckless com metade da velocidade |
| Build | release com `lto="fat"`, `codegen-units=1`, `panic="abort"`; Makefile wrapper com `EXE=` (OpenBench); binários por nível x86-64 (v3 = AVX2 para testers) | requisito do OpenBench/CCRL |
| Casas | LERF (A1=0 … H8=63) | casa com Fathom/Pyrrhic e Stockfish |
| Tabuleiro | `pieces[6]` + `colors[2]` + mailbox `[Option<Piece>; 64]` | padrão do topo |
| Make/unmake | **copy-make** de um estado compacto numa pilha (Viridithas, Stormphrax) | undo = pop; teste de propriedade trivial |
| Chaves | Zobrist 64 bits com seed fixa + desde já pawn key e non-pawn keys | bench determinístico; correction history vai precisar |
| Sliders | **fancy magic bitboards** (sem PEXT) | Ryzen 5 3600 é Zen 2: PEXT em microcódigo, 18 ciclos |
| Lance | 16 bits sobre `NonZeroU16` (`Option<Move>` de graça) | padrão; niche de Rust |
| Roque | "rei captura a própria torre" internamente; converte para e1g1 só na saída quando `UCI_Chess960=false` | cobre FRC/DFRC sem refatorar |
| FEN | aceitar KQkq, X-FEN e Shredder-FEN; campos halfmove/fullmove opcionais | suítes de perft e python-chess usam todos |
| En passant | só entra no hash se houver captura ep **legal**; aceitar ep "sempre" vindo da GUI sem hashear | python-chess manda ep após todo lance duplo |
| Movegen | pseudo-legal por categoria + `is_legal` preguiçoso + **`is_pseudo_legal` rigoroso para o lance da TT** | a Coda registrou "centenas de Elo" perdidos e bot abandonando por PV ilegal |
| Move picker | em estágios desde a v1 (TT → capturas boas → quiets → capturas ruins) | evita gerar quiets quando uma captura já corta |
| UCI | stdin em thread própria; `isready` respondido até durante a busca; `bestmove` sempre; `info` final antes; mate em lances; `position` reconstrói posição **e histórico de chaves** | regras literais da spec de 2006 |
| Opções | `Hash`, `Threads` (pode ser 1..1 no início), `Move Overhead`, `UCI_Chess960`, `MultiPV`, `SyzygyPath` (no-op até existir) | OpenBench e lichess-bot |
| Comandos extras | `bench` (≈50 posições fixas incluindo FRC/DFRC, depth fixa, 1 thread, hash 16, saída `Bench: N nodes M nps`), `go perft N` com divide, `d` | OpenBench lê o bench da mensagem do commit |

**TDD da base (perft em três camadas):** (a) 6 posições canônicas da CPW em profundidade baixa
no `cargo test`; (b) `standard.epd` (128 posições) e `fischer.epd` (960 FRC) em teste de
integração `--release`; (c) `go perft N` com divide para diff contra o `go perft` do Stockfish,
que também gera números para DFRC.

Números oficiais (startpos): 20 · 400 · 8.902 · 197.281 · 4.865.609 · 119.060.324 ·
3.195.901.860. Kiwipete: 48 · 2.039 · 97.862 · 4.085.603 · 193.690.690.

---

## 5. Roteiro de construção com metas

Ordem de busca = "Improved Connorpasta" (chessprogramming.org/Search_Progression), validada
pelas séries de Elo medido por feature de Clockwork (HCE, 2025), Reckless (NNUE, 2025) e
Sirius (remoção, engine madura). Os números abaixo são **prioridade relativa**, não promessa:
SPRT com parada antecipada infla ganhos e o valor depende do contexto.

### Fase 0 — Fundação (antes de qualquer busca)
Tabuleiro, movegen, perft 100%, FEN, Zobrist, repetição/50 lances, UCI completo, `bench`,
CI (perft + bench + partidas curtas de fastchess num build de debug falhando em
`illegal move`/`disconnect`/`stall`), fastchess local, random-mover contra si mesmo como
"sanity check" da infraestrutura.

### Fase 1 — v1 jogável → Lichess
negamax fail-soft + alpha-beta + iterative deepening → quiescence (stand pat, evasões em xeque)
→ MVV-LVA → TT (TT move na ordenação, cortes só em non-PV, mate ajustado por ply, guardar TT
move no fail-low) → PVS → aspiration windows → gestão de tempo soft/hard (soft = t/20 + inc/2,
hard = t/4) → repetição/50 lances → check extension. Avaliação: PSTs no formato PeSTO (tapered).

Elo medido na adição (Clockwork/Reckless): QS ~+190 · MVV-LVA +300 a +600 · TT move ~+190 ·
TT cutoffs +60/+100 · TM soft/hard +54 · AW +10/+33 · PVS +11/+27.
**Meta: ~2.000–2.600; stress test de tempo a 2+0.02 obrigatório antes de ir ao ar.**

### Fase 2 — O grosso do Elo de busca
RFP (+57/+73) → NMP (+86; R dinâmico +61) → butterfly history com malus e gravity (+93/+104)
→ killers (+17) → LMR com fórmula log (+34/+128; só rende depois da history) → LMP (+39) →
futility → IIR (+43) → improving → SEE (ordenação +62, pruning no QS +43, no search +24)
→ move picker staged completo (+30).
**Referência:** Leorik foi de ~2.100 a ~2.530 com esse bloco e avaliação só de PST.
**Em paralelo:** datagen próprio dentro do binário (ver Fase 3).

### Fase 3 — Primeira NNUE (dados próprios)
- Datagen: 8–9 plies aleatórios com filtro SEE (opcionalmente posições DFRC), descartar abertura
  com |eval| > 1000, 5k nós soft por lance (subir para 20–25k com mais CPU), adjudicar vitória
  |score| ≥ 2500 por 4 plies e empate |score| ≤ 4 por 12 plies, gravar em `viriformat`.
- Volume: 100M+ para a 1ª rede; ~1B para uma boa rede de 256–512 neurônios.
- Rede: (768 → 128 ou 256)×2 → 1, perspectiva, SCReLU, QA = 255, QB = 64, escala 400; receita
  `examples/progression/1_simple.rs` do `bullet` (AdamW, batch 16.384, 40 superbatches, LR
  cosine, WDL 0,25–0,75 calibrado por SPRT). **Embaralhar sempre.**
- Inferência: pilha de acumuladores com atualização incremental e AVX2 desde o início (o Leorik
  ingênuo fazia 50K nps contra 5M da HCE).
- Testes antes da inferência: incremental == reconstrução completa após make/unmake aleatórios
  (roque, ep, promoção); quantizada ≈ referência float; simetria de cores.
- Ganho documentado da 1ª rede sobre a HCE do próprio autor: **+125 a +360**.
**Meta: ~3.000–3.300.**

### Fase 4 — De ~3.300 a ~3.500
Continuation history 1/2/4 ply, capture history, history pruning; **correction history** (pawn
→ non-pawn → minor/major → continuation; +10 a +43 por variante em engines jovens); singular
extensions + multicut + double/negative (testar **em LTC**: SE deu −2,6 STC / +5,7 LTC no
Clockwork); melhorias do QS; razoring/ProbCut; TT com clusters/aging/prefetch (+46/+56 sob
pressão de hash); TM por nós e estabilidade do melhor lance; Lazy SMP com thread voting.
Rede: 256 → 512 → 1024, king buckets com espelhamento horizontal + Finny tables, 8 output buckets.
SPSA de parâmetros de busca; Syzygy (Fathom/Pyrrhic); OpenBench.

### Fase 5 — Rumo ao top-20
Rede multicamada (L1 pairwise → 16/32 → 32 → 1 por output bucket, int8), threat inputs e
pawn-pair inputs (a fronteira de 2026; o Stockfish 18/19 adotou ideias de engines pequenas),
5–15 bilhões de posições, datagen distribuído, ttPv/cutNode explícitos, LMR fracionária com
dezenas de termos tunados por SPSA, histories compartilhadas entre threads.

### Metas realistas
| Horizonte | Meta | Condição |
|---|---|---|
| dias–semanas | v1 no Lichess, ~2.000–2.900 | base sem bugs |
| 1–2 meses | 1ª NNUE, ~3.000–3.300; entrar na CCRL | 100M+ posições próprias |
| 6–12 meses | ~3.500 | compute além do desktop (servidor/OpenBench compartilhado) |
| 2–3 anos | top-20/top-15 (~3.650+ na 40/15) | bilhões de posições, centenas de threads, inovação própria |
| — | vencer o Stockfish | ninguém fez desde 2020; não é meta com prazo |

---

## 6. Como vamos melhorar (o processo)

1. **Uma ideia por branch** (`feat/…`). Commit funcional termina com `Bench: N`; não funcional
   com "No functional change".
2. **SPRT com fastchess** (pentanomial, Elo normalizado), alpha = beta = 0,05 (ou beta = 0,10
   para acelerar — decidir e registrar):

   | Fase | Ganho | Não-regressão | Livro |
   |---|---|---|---|
   | < ~2.500 | [0, 10] | [−10, 0] | 8moves_v3 |
   | até top-200 | [0, 5] | [−5, 0] | UHO_Lichess_4852_v1 / Pohl |
   | top-30 | [0, 3] | [−3, 1] | UHO |

   STC 8+0.08 (Hash 16), LTC 40+0.4 (Hash 64–128). Busca, extensões, tempo e tunes confirmam
   em LTC antes do merge. Mudanças de TT testam com Hash = 1 MB; SMP com 4–8 threads.
3. **Custo real de um teste (partidas, média para um patch neutro — a maioria das ideias
   falha):** [0,10] ≈ 6.400 · [0,5] ≈ 25.600 · [0,3] ≈ 71.000. No pior caso, ~1.046.535/(largura)².
4. **Merge só com o bloco do SPRT colado no PR** (Elo, LLR, partidas, penta) — registro auditável.
5. **Força absoluta:** gauntlet periódico contra a escada Stash (v20 ≈ 2.500 … v37 ≈ 3.419) e,
   depois, as listas públicas. Self-play infla: só ~60% do ganho aparece contra outras engines.
6. **Ledger de testes e de redes** (commit, bench, bounds, TC, modelo de Elo, resultado; qual
   rede gerou quais dados).
7. **SPSA** quando houver ~30+ parâmetros (R_end 0,002; C_end ≈ faixa/20); sempre validado por
   SPRT em STC e LTC.
8. **Invariantes em TDD, Elo em SPRT.** Testes obrigatórios: perft, hash incremental == hash do
   zero (inclusive pawn/non-pawn), round-trip de mate na TT, repetição/50 lances, suíte de SEE,
   simetria da avaliação, NNUE incremental == completa, bench determinístico.

---

## 7. v1 enfrentando outros — Lichess passo a passo

1. **Checklist UCI** coberto por testes, incluindo `go movetime` (o lichess-bot usa
   `movetime 10000` no 1º lance de cada cor), `position fen` e saída de `info … score cp`.
2. **Harness E2E próprio**: o `test_uci()` do lichess-bot é roteirizado (exige mate em 4) e não
   serve direto; reaproveitar o Lichess falso (`test_bot/lichess.py` + `run_bot`) com oponente de
   lances legais aleatórios e asserção "partida termina sem lance ilegal nem derrota por tempo".
3. **Conta:** nova, com o nome definitivo, **sem nenhuma partida** (nem contra a IA do site).
   Token em `https://lichess.org/account/oauth/token/create?scopes[]=bot:play`, guardado na
   variável `LICHESS_BOT_TOKEN` (fora do git). `python lichess-bot.py -u` **uma vez** —
   irreversível.
4. **lichess-bot 2026.8.9.2**, Python ≥ 3.11 (você tem 3.14, que o CI deles testa), num venv.
5. **config.yml da v1:** `uci_options: {}` (as opções padrão — Move Overhead, Threads, Hash,
   SyzygyPath, UCI_ShowWDL — derrubam a engine com EngineError se ela não as declarar);
   `ponder: false` (vem `true`); livro polyglot pequeno com `weighted_random`; **desligar
   `online_moves`** (cloud/chessdb jogariam lances do Stockfish); `resign_enabled: false`;
   aceitar só ritmos com incremento; `quit_after_all_games_finish: true`; `pgn_directory`.
6. **Primeiras ~50–100 partidas em casual.** Todo bot novo começa com **3000 provisório**: o
   matchmaking vai desafiar bots Stockfish e a v1 vai perder muito até assentar — usar
   `opponent_max_rating` fixo no começo.
7. **Limite de 100 partidas bot×bot por dia**, contando desafios **recebidos**. Controlar com
   `challenge_timeout` 8–10 min, `max_recent_bot_challenges`, `preference`/`games_reserved_for_humans`.
8. **Sem compensação de lag para bots** e servidores na França: ~200 ms por lance saem do relógio
   a partir do Brasil. Preferir incremento; `move_overhead` começa em 2000 ms e desce pelos logs.
9. **Detector de farming:** partida rated com os mesmos 20 primeiros **plies** e mesmo vencedor
   que uma das 2 anteriores contra o mesmo oponente não mexe no rating → o livro precisa variar
   dentro dos 10 primeiros lances.
10. **24/7:** no Windows via NSSM (o WSL não se mantém vivo sozinho); depois VPS na Europa (§8).
    Arenas só quando o organizador libera bots (entrar via API ou usar o BotLi, que entra sozinho);
    Swiss é bloqueado; UltraBullet é proibido.
11. **O rating do Lichess não mede força de engine** acima de ~2.500–2.700 (comprime; o topo dos
    bots fica em 3.100–3.400, quase todo Stockfish). É vitrine e smoke test; a régua é SPRT +
    CCRL.

---

## 8. Servidores, hardware e custos

Preços vistos em 26/09/2026 nas páginas/APIs oficiais e conferidos por um segundo agente.
Câmbio: US$ 1 = R$ 5,19; € 1 = R$ 5,92 (PTAX 25/09). **Não incluem IOF de cartão (3,5%)**.
2026 teve crise de DRAM: a Hetzner reajustou em 15/06 (CCX13 +169%), então tabelas antigas
estão erradas. Conceito que decide tudo: **teste de engine precisa de núcleo físico dedicado e
homogêneo** — vCPU compartilhada, SMT contado como núcleo e CPUs híbridas P/E geram ruído.

### 8.1 Sua máquina hoje
- AMD Ryzen 5 3600 (Zen 2, 6C/12T, AVX2, **sem AVX-512, PEXT lento**), 16 GB, GTX 1660 6 GB.
- **Testes:** ~146 partidas/h por núcleo a 8+0.08 (fórmula do fishtest: 24,7 s/partida; 40+0.4
  = ~29/h). Com concorrência 5 → **~730 partidas/h**. Tamanho real de SPRT nas instâncias
  OpenBench públicas: engine jovem em [0,5] fecha com mediana de 6–8k partidas (≈ 55 núcleo-h
  → **~9 h na sua máquina**); engine madura em [0,2]/[0,3] precisa de 40–90k (≈ 515 núcleo-h →
  **~3,6 dias**). Ou seja: o PC atual dá conta das fases 0–3; da fase 4 em diante vira gargalo.
- **Datagen:** estimativa ~0,6M posições/h por núcleo a 5k nós → **~80M/dia** usando os 6
  núcleos 24/7 (medir no primeiro datagen). 1ª rede (100M+) em 1–2 dias; 1B em ~12 dias.
- **Treino da rede:** o `bullet` roda **só em GPU** (CUDA/ROCm/Metal — conferido no repositório
  em 26/09/2026; o backend de CPU foi removido em abr/2026). A GTX 1660 (Turing, sm_75) é
  suportada pelo CUDA 13 e o `bullet` compila os kernels para a GPU presente, em FP32, sem
  tensor cores. Há um relato antigo de erro de build com 1660 (issue #336) → **primeira coisa a
  testar** rodando `examples/progression/1_simple` e anotando o `pos/sec`. Estimativa (sem
  benchmark publicado): redes pequenas (128–256, 40–100 superbatches) em horas; 512 neurônios
  com 320 superbatches em ~8–15 h.

### 8.2 GPU para os treinos grandes (por hora, só quando precisar)

| Opção | Preço | Observação |
|---|---|---|
| RunPod Community — RTX 3090 / 4090 | US$ 0,22 / 0,34 por hora | melhor custo por ciclo |
| Vast.ai — RTX 3090 / 4090 (hosts verificados) | ~US$ 0,20 / ~0,54 por hora (medianas) | marketplace; conferir banda (~US$ 0,0026/GB) |
| Kaggle | grátis, ~30 h/semana de T4, sessões de 12 h | só 4 núcleos de CPU → o loader do `bullet` limita |
| Modal | US$ 30/mês de crédito grátis | cobrança por segundo |
| A100/H100, Lambda, DigitalOcean, Hetzner GEX45 | 3–10× mais caro por ciclo | treino de NNUE é FP32 e limitado por memória/loader; não compensa |
| **Colab gratuito** | **proibido para este uso** | o FAQ lista "chess training" e "distributed computing workers" como proibidos |

Custo estimado de um ciclo de 320 superbatches numa 3090/4090 alugada: **~R$ 4–10**.

### 8.3 Bot 24/7

Os servidores do Lichess ficam na **OVH em Gravelines (França)** — confirmado na planilha
oficial de custos e por medição de ping: Gravelines 0,1 ms · Paris ~5 ms · Frankfurt ~9 ms ·
Nuremberg ~13 ms · **São Paulo ~203 ms** (≈ 13% do relógio numa partida 1+0, porque bot não
tem compensação de lag).

| Fase | Opção | Custo | Observação |
|---|---|---|---|
| v1 (testes) | seu PC, como serviço via NSSM | R$ 0 marginal | 200 ms de latência; disputa núcleos com testes/datagen |
| v1 24/7 grátis | Oracle Always Free A1 (ARM, 2 OCPU/12 GB), home region **Paris** | R$ 0 | exige build aarch64/NEON; home region não muda depois; pode faltar capacidade; instância ociosa é recuperada |
| v1 24/7 pago | OVH VPS-1 em Gravelines (2 vCores, 4 GB) | € 4,49/mês sem compromisso (~R$ 27); € 3,81 com 12 meses pagos adiantado | mesmo datacenter do Lichess; x86 (mesmo binário dos testes) |
| fase competitiva | netcup RS 500 (2 núcleos dedicados EPYC 9645, AVX-512) | ~€ 11–12,50/mês em contrato de 12 meses (~R$ 65–74) | sem "steal" de CPU; ~13 ms |

Evitar: Hetzner Cloud CX (indisponível) e CCX (caro após o reajuste); VPS no Brasil (latência).

### 8.4 CPU para testes e geração de dados

| Opção | Núcleos | Preço | ≈ R$/mês | Quando |
|---|---|---|---|---|
| Instância OpenBench comunitária (FuryBench, SweHosting…) | centenas de threads compartilhadas | grátis | 0 | por convite no Discord do OpenBench; contrapartida = doar workers |
| **Upgrade local: Ryzen 9 5900XT** (AM4, se a BIOS da sua placa aceitar) | 16 Zen 3 (AVX2, PEXT rápido) | R$ 2.049,99 no PIX + cooler (vem sem) | ~R$ 150–240 amortizado com energia | ~3× o throughput atual; o hardware é seu |
| Upgrade AM5 (9950X + placa + DDR5) | 16 Zen 5 (AVX-512) | ~R$ 6,2–8,2 mil (DDR5 a ~R$ 3.900/32 GB) | — | só quando a DDR5 baratear |
| OVH RISE-S (Ryzen 7 9700X) | 8 Zen 5 | US$ 77/mês + instalação de 1 mês | ~R$ 400 | também serve de servidor OpenBench |
| OVH RISE-L (Ryzen 9 9950X) | 16 Zen 5 | US$ 177/mês | ~R$ 920 | fase 4 |
| Hetzner AX102-1-LTD (7950X3D) | 16 Zen 4 | € 157,30 + € 39 setup | ~R$ 930 | estoque limitado; núcleos assimétricos (V-Cache) |
| **OVH RISE-XL (EPYC 9455)** | 48 Zen 5 | US$ 354/mês | ~R$ 1.840 | melhor preço por núcleo (~R$ 38/núcleo) |
| Hetzner AX162-1-LTD (EPYC 9454P) | 48 Zen 4 | € 317,30 + € 39 setup | ~R$ 1.880 | estoque limitado |
| Spot AWS m7a/c7a.48xlarge (Genoa) | 192 físicos | US$ 1,32–1,70/h (eu-south-2 / us-west-2) | por uso | rajadas de datagen: **~R$ 60 por 1 bilhão de posições** (estimativa); interrompível; conta nova tem cota baixa |
| Spot Azure F64als_v7 / HB176rs_v4 | 64 / 176 físicos | US$ 0,46–0,75/h / US$ 1,33/h | por uso | idem |
| GitHub Actions (repo público) | 4 vCPU | grátis | 0 | só CI (build, perft, bench, smoke-test) |

- OVH cobra em USD sem imposto para o Brasil; o desconto de 12 meses exige pagar o ano adiantado.
- Evitar para testes: Hetzner Cloud CCX, vCPU compartilhada, CPUs híbridas (i9-13900, Core
  Ultra), provedores brasileiros e Latitude.sh (caros por núcleo; latência não importa aqui).
- Servidor próprio do OpenBench: o plano grátis do PythonAnywhere deixou de ser viável (contas
  novas sem MySQL desde jan/2026) → plano Developer (US$ 10/mês) ou uma VPS pequena.

### 8.5 Faixas de orçamento

| Faixa | O que compra | Serve até |
|---|---|---|
| **R$ 0** | PC local (SPRT jovem ~9 h, ~80M posições/dia), 1660 para redes pequenas, Kaggle, bot no PC ou no Oracle Free, CI no GitHub | fases 0–3 (v1 → 1ª NNUE, ~3.000–3.300) |
| **~R$ 100–300/mês** | + spot por hora para datagen, GPU por hora para treinos maiores, VPS do bot em Gravelines (~R$ 27), servidor OpenBench (US$ 10) — ou, no lugar, a compra única do 5900XT | fase 4 inicial |
| **~R$ 900–1.900/mês** | dedicado de 16–48 núcleos Zen 5 na Europa rodando OpenBench + workers + bot | fase 4–5 (SPRTs [0,3] de 40–90k partidas) |
| **alternativa** | entrar numa instância OpenBench comunitária doando o seu PC como worker | qualquer fase, se aceito |

**Recomendação:** começar em R$ 0. O primeiro gasto que faz sentido é GPU/spot por hora
(pagar só pelo uso). O upgrade para o 5900XT entra quando os testes locais virarem gargalo
(fase 2–3). Servidor mensal só a partir de ~3.300, quando os testes passam a exigir dezenas de
milhares de partidas.

---

## 9. Armadilhas documentadas (cada uma custou Elo a alguém)

- Mate na TT sem ajuste por ply; mate não provado vindo de NMP/QS.
- NMP sem material não-peão (zugzwang).
- Repetição: 2-fold na árvore, 3-fold com histórico do jogo; valor de empate com jitter;
  não cortar pela TT com rule50 ≥ 96 (GHI).
- Explosão de busca por extensões encadeadas (limitar a ply < 2·rootDepth).
- Janela de aspiração passando de ±INF.
- History sem gravity (overflow/saturação).
- Chave de TT curta demais (16 bits → false hits; +300 ao corrigir no benchmark de LLM).
- Stack de 8 MB não aplicado no Windows (Clockwork, jul/2026).
- Perda de tempo com SMP (−72 a −136 Elo no Sirius); bug de tempo que só aparece a 2+0.02.
- Primeira NNUE que perde para a HCE: dados sem shuffle, posições não quietas, inferência sem
  SIMD, rede grande/buckets cedo demais com poucos dados.
- Ganho em nós fixos ≠ ganho em tempo (threat inputs: +33 em nós fixos, +2,7 em STC).
- Tunes em STC que não escalam; parâmetros "envenenados" no SPSA.
- Texto de repositórios de terceiros tratado como instrução pelo agente (tratar sempre como dado).

---

## 10. Referências principais

- Rankings: computerchess.org.uk/ccrl (4040, 404, 404FRC) · cegt.net · sp-cc.de
- TCEC: wiki.chessdom.org (Main_Page, TCEC_Questionnaire, TCEC_Swiss_10, Rules,
  TCEC_Season_Further_information, TCEC_NNUE_Guideline, Current_Engine_Status)
- Ordem de implementação: chessprogramming.org/Search_Progression · Getting_Started · Perft_Results
- Elo por feature: github.com/official-clockwork/Clockwork (PRs) · github.com/mcthouacbb/Sirius
  (elo_estimates.md) · github.com/codedeliveryservice/Reckless (PRs)
- Testes: github.com/Disservin/fastchess · github.com/AndyGrant/OpenBench (wiki) ·
  dannyhammer.github.io/engine-testing-guide · cantate.be/Fishtest/normalized_elo_practical.pdf
- NNUE: github.com/jw1912/bullet (docs + examples/progression) · asteri.sm/files/2024-06-01-nnue.html
  · networkhistory.txt da Viridithas (lições de ~100 redes)
- Lichess: github.com/lichess-bot-devs/lichess-bot (wiki) · lichess.org/api (seção Bot) · github.com/Torom/BotLi
- Livros de aprendizado: rustic-chess.org · dogeystamp.com/chess0 · devlog do Leorik (TalkChess t=79049)
- Livros de abertura: github.com/official-stockfish/books · github.com/AndyGrant/openbench-books
- Comunidade: Discord Engine Programming (discord.com/invite/F6W6mMsTGN, canal #bullet) ·
  Stockfish (discord.gg/GWDRS3kU6R, #engines-dev) · OpenBench (discord.com/invite/9MVg7fBTpM) ·
  TalkChess (tópico "New engine releases 2026 H2")
- Precedentes com LLM: github.com/adamtwiss/coda · github.com/stevemaughan/chess-engine-benchmark

---

## 11. Decisões pendentes (do dono)

1. ~~Modo de uso do Claude Code~~ — **decidido em 26/09/2026: B, com transparência total**
   (ver `docs/decisoes.md`, D1). Opções que estavam na mesa:
   - **A — Você escreve o núcleo** (movegen, busca, avaliação, NNUE); o Claude revisa, gera
     testes, explica, faz infraestrutura (CI, scripts de SPRT, datagen tooling). Elegível às
     ligas do TCEC; bem mais lento.
   - **B — O Claude escreve, você dirige, revisa e decide** (modelo Coda). Muito mais rápido;
     hoje cai na Cat 4 do TCEC (fora das ligas regulares na S30), CCRL/Lichess normais com
     rótulo "AI assisted".
   - Em qualquer caso: declarar com honestidade (AI_USAGE.md, questionário, anúncio).
2. Dados de treino: 100% próprios (recomendado) ou Lc0.
3. Licença: GPL-3.0 (recomendado).
4. Nome da engine e da conta BOT (o upgrade é irreversível).
5. Quality gate do projeto (proposta: `cargo test` com perft, `cargo clippy -D warnings`,
   `cargo fmt --check`, bench determinístico conferido no CI, partidas curtas de fastchess em
   debug).
6. Orçamento mensal para compute (§8).
