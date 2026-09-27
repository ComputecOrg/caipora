# Registro de decisões (rulings)

Cada decisão: o que foi decidido, por quê e quanto custa se estiver errada.

## D1 — 26/09/2026 — Modo de desenvolvimento: B, com transparência total

- **Decisão:** o Claude Code escreve o código; o dono dirige, revisa e decide. O uso de IA é
  declarado em todos os lugares onde é perguntado ou relevante: README, `AI_USAGE.md`, anúncios
  na CCRL/TalkChess ("AI assisted") e questionário do TCEC (pergunta 5). Os commits mantêm a
  coautoria do Claude.
- **Por quê:** o dono está começando do zero em programação de xadrez, e o modo B é muito mais
  rápido (precedente: a Coda, escrita pelo Claude Code, chegou ao #11 da CCRL 40/15 em ~8 meses).
  A regra de IA do TCEC é declaradamente um teste da Season 30 e pode mudar. Esconder o uso de IA
  foi descartado: o TCEC e a CCRL perguntam diretamente, a comunidade faz análise forense, e o
  precedente para declaração falsa é exclusão retroativa (Houdini, Fire) ou banimento (Rybka).
- **Custo se estiver errada:** enquanto a regra da S30 valer, a engine fica fora das ligas
  regulares do TCEC (só Swiss via playoff Cat 3+4 e gauntlet de IA separado). Mudar para o modo A
  depois exigiria reescrever o núcleo à mão. CCRL, CEGT, SPCC e Lichess não são afetados, apenas
  rotulam a engine.

## D2 — 26/09/2026 — Dados de treino da NNUE: 100% próprios

- **Decisão:** toda rede é treinada só com partidas geradas pela própria engine (self-play). Nada
  de dados do Lc0, do Stockfish ou redes de terceiros. Manter um log de gerações (qual rede gerou
  quais dados, quantas posições, quantos nós por lance).
- **Por quê:** é a norma de prestígio da comunidade e protege o projeto caso surja uma competição
  de IA com regras sobre a origem dos dados. Com o modo B a vantagem no TCEC é menor (já estamos
  na categoria de IA), mas trocar de política depois obrigaria a regerar tudo.
- **Custo se estiver errada:** a rede fica forte mais devagar que a da Coda (que usou dados do
  Lc0). Gerar bilhões de posições consome CPU: ~80M posições/dia na máquina atual.

## D3 — 26/09/2026 — Linguagem: Rust estável

- **Decisão:** Rust estável, edition 2024, com a versão do toolchain fixada no repositório. Nada
  que dependa do nightly.
- **Por quê:** Reckless (#2) e Coda são Rust; o treinador `bullet` é Rust; `cargo test` sustenta
  o TDD; o build é igual em Windows e Linux (o TCEC roda Linux).
- **Custo se estiver errada:** há menos código de referência que em C++ (15 das 25 primeiras
  engines são C++). Diferença de força por linguagem medida em ~15–26 Elo; reescrever custaria
  semanas.

## D4 — 26/09/2026 — Licença: GPL-3.0

- **Decisão:** GPL-3.0-or-later. Bibliotecas externas só MIT/BSD/GPL-compatíveis e declaradas.
  Nunca portar código de outras engines: ideias sim, código e constantes não.
- **Por quê:** padrão do topo; compatível com Fathom/Pyrrhic (MIT) e com o `bullet` (MIT).
- **Custo se estiver errada:** mudar para uma licença mais permissiva depois exigiria o
  consentimento de todo colaborador externo (hoje só o dono).

## D5 — 26/09/2026 — Quality gate

- **Decisão:** toda mudança passa, no CI e antes do merge, por: (1) `cargo test`, incluindo as
  suítes de perft; (2) `cargo clippy` com warnings tratados como erro; (3) `cargo fmt --check`;
  (4) `bench` determinístico batendo com o `Bench: N` do commit, mais partidas curtas de
  fastchess num build de debug falhando em lance ilegal, travamento ou derrota por tempo.
  O item (4) entra quando existir busca.
- **Por quê:** invariantes se provam com testes; força se mede com SPRT. O SPRT sozinho já
  aprovou código com bug (Clockwork, +29,5 Elo com a chave de peões errada).
- **Custo se estiver errada:** CI mais lento, estimado em alguns minutos por mudança.
- **Revisão (26/09/2026, pedido do dono):** o CI roda **só em Linux** (sistema do TCEC e runner
  mais barato; em repositório privado o Windows consome minutos em dobro). O Windows fica coberto
  pelo gate local (`scripts/gate.sh`), porque a máquina de desenvolvimento é Windows. Um job de
  build Windows (.exe para a CCRL) entra quando houver o primeiro release. Custo se estiver
  errada: um bug que só aparece no Windows pode escapar se alguém commitar sem rodar o gate local.

## D6 — 26/09/2026 — Nome: Caipora

- **Decisão:** a engine se chama **Caipora** (crate/binário `caipora`). A conta BOT no Lichess
  segue o mesmo nome (ex.: `CaiporaBot`), criada só na v1.
- **Por quê:** escolha do dono. Em 26/09/2026 o nome estava livre na CCRL (lista Blitz completa),
  no GitHub (busca por engine de xadrez) e no Lichess (`CaiporaBot` e `Caipora_BOT`).
- **Custo se estiver errada:** renomear o código é barato; a conta do Lichess não muda depois do
  upgrade para BOT.

## D8 — 26/09/2026 — Avaliação da v1 com tabelas próprias (sem PeSTO)

- **Decisão:** a avaliação da v1 usa material e tabelas de posição **geradas por fórmulas
  próprias** (centralização, avanço de peão, abrigo do rei, sétima fileira da torre),
  interpoladas por fase. Nada das tabelas PeSTO nem de outra engine. O próximo passo é ajustar os
  pesos com Texel tuning sobre partidas do próprio Caipora e, depois, trocar pela NNUE (D2).
- **Por quê:** o `AI_USAGE.md` promete que constantes e tabelas de outras engines nunca são
  copiadas; as tabelas PeSTO, recomendadas na pesquisa, são constantes de outra engine (RofChade).
  Fórmulas geradas no código deixam a origem clara.
- **Custo se estiver errada:** a v1 fica mais fraca que com PeSTO (estimativa sem medição: algo
  como 100 a 200 Elo). O ajuste com dados próprios recupera isso.

## D7 — 26/09/2026 — Repositório privado até a primeira submissão pública

- **Decisão:** o repositório fica na conta **ComputecOrg** do dono no GitHub
  (`ComputecOrg/caipora`, confirmado pelo dono em 26/09/2026), **privado** durante o
  desenvolvimento, e vira público **antes da primeira submissão** ao TCEC (obrigatório) e,
  preferencialmente, antes do anúncio na CCRL. Commits assinados por
  `Matheus de Carvalho Jesus <euescolhoesse@gmail.com>`, com a coautoria do Claude.
- **Por quê:** o Lichess não exige código aberto (a conta BOT é independente do código), e a
  CCRL aceita engines fechadas com executável. O TCEC exige código aberto e compilável a partir
  do repositório para engines da categoria de IA (regra do Swiss 10). Como o código é todo nosso,
  a GPL não obriga a publicar enquanto não publicarmos.
- **Custo se estiver errada:** o CI em repositório privado consome a cota do plano Free
  (2.000 min/mês; runners Windows contam em dobro). Engine fechada e feita com IA tende a atrair
  mais desconfiança da comunidade, e ao abrir o repositório todo o histórico fica visível.

## D9 — 27/09/2026 — Teste de tempo com relógio, dividindo a máquina

- **Decisão:** o SPRT da regra dos 60% (PR #8) roda a 8+0.08 com `-concurrency 3`, ao mesmo
  tempo que SPRTs de nós fixos com `-concurrency 2` e o bot do Lichess (1 núcleo quando joga).
  Limites [0, 10]. Se passar de ~8000 partidas sem decisão, a regra entra só se o placar não for
  negativo (é a prática padrão e a primeira rodada deu +8), registrando isso no PR.
- **Por quê:** o dono pediu para começar o teste assim que o bot subisse, sem esperar a
  madrugada; testes de nós fixos não sofrem com disputa de CPU, e a disputa afeta as duas
  engines do teste de relógio por igual.
- **Custo se estiver errada:** ruído a mais no teste de relógio (mais partidas até decidir) e, no
  pior caso, perdas por tempo falsas. As terminações são conferidas no PGN antes de aceitar.

## D10 — 27/09/2026 — Branches da fase 3 empilhados

- **Decisão:** cada melhoria de busca da fase 3 nasce em cima da anterior (correction history →
  continuation history → …) e é testada contra o binário da anterior. O PR de cima aponta para o
  branch de baixo e é redirecionado para a `main` quando o de baixo entra.
- **Por quê:** testar cada uma contra a `main` e juntar depois mede combinações que ninguém
  testou, e cada merge exigiria um novo commit de bench.
- **Custo se estiver errada:** se uma de baixo falhar no SPRT, as de cima precisam de rebase e de
  novo SPRT (algumas horas de CPU).

## D11 — 27/09/2026 — Primeira NNUE sem passar pelo Texel tuning

- **Decisão:** pular o ajuste da avaliação à mão (Texel tuning, previsto na D8) e ir direto para a
  NNUE. Dados do `caipora datagen`: 5000 nós por lance, 8 lances aleatórios na abertura (abertura
  acima de 1000 cp é descartada), só posições quietas (fora de xeque, melhor lance quieto,
  |pontuação| < 2500), vitória adjudicada com 4 meios-lances a ±2500, empate adjudicado depois do
  meio-lance 80 com 8 meios-lances dentro de 10 cp.
- **Por quê:** a NNUE substitui a avaliação à mão, então o trabalho de ajustá-la se perde; os dados
  gerados com a avaliação atual já trazem o sinal que importa (o resultado das partidas), e as
  gerações seguintes usam a própria rede.
- **Custo se estiver errada:** a primeira rede aprende de pontuações de uma avaliação fraca e sai
  pior do que sairia com dados de uma avaliação ajustada; a segunda geração de dados (com a rede)
  corrige isso, ao custo de ~1 dia de CPU a mais.

## D12 — 27/09/2026 — Build x86-64-v3 e ganhos só de velocidade sem SPRT

- **Decisão:** o build padrão passa a ser `target-cpu=x86-64-v3` (AVX2), em `.cargo/config.toml`.
  Mudança que só acelera, com o **mesmo bench** (mesmos nós, mesma busca) e ganho de nós/s medido
  em rodadas alternadas, entra sem SPRT.
- **Por quê:** mesmo número de nós prova que a busca não mudou; o ganho de velocidade só pode
  somar Elo. A CPU é o gargalo do projeto e SPRT de relógio custa horas. Medido: +7% de nós/s com a
  avaliação à mão e +23% com a NNUE (e mais 3% com o produto em 16 bits na saída da rede).
- **Custo se estiver errada:** o binário não roda em CPU sem AVX2 (anteriores a 2013); para esses,
  compilar com `RUSTFLAGS="-C target-cpu=x86-64"`. Um ganho de velocidade medido errado custaria
  pouco Elo e apareceria no próximo gauntlet.
