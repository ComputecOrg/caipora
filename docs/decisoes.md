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
