# Colocando o Caipora no Lichess (conta BOT)

O lichess-bot já está instalado em `C:\Projetos\lichess-bot` (versão 2026.8.9.2, Python 3.14,
venv em `venv\`). O `config.yml` é gerado e validado pelo script `scripts/lichess_config.py`.
Falta só o que precisa ser feito por você: a conta e o token.

## 1. Conta nova (irreversível)

1. Crie uma conta **nova** no Lichess, com o nome definitivo (ex.: `CaiporaBot`). **Não jogue
   nenhuma partida com ela**, nem contra a IA do site: conta com partida não pode virar BOT.
2. Logado nela, gere um token em
   <https://lichess.org/account/oauth/token/create?scopes[]=bot:play&description=caipora>
   (escopo `bot:play`). O token só aparece uma vez.
3. Guarde o token numa variável de ambiente do Windows, fora do git:
   ```powershell
   setx LICHESS_BOT_TOKEN "lip_xxxxxxxxxxxxxxxx"
   ```
   Feche e abra o terminal para a variável valer.

## 2. Gerar o config e virar BOT

```powershell
cd C:\Projetos\ChessAI
cargo build --release
cd C:\Projetos\lichess-bot
venv\Scripts\python.exe C:\Projetos\ChessAI\scripts\lichess_config.py --engine C:\Projetos\ChessAI\target\release\caipora.exe
venv\Scripts\python.exe lichess-bot.py -u      # converte a conta em BOT (uma vez, sem volta)
venv\Scripts\python.exe lichess-bot.py -v      # começa a jogar
```

O script já deixa:
- só opções que o Caipora declara (`Hash 128`, `Move Overhead 100`), sem ponder;
- livros online, tablebases online e "cloud analysis" **desligados** (senão o bot jogaria lances
  do Stockfish e o resultado não mediria a nossa engine);
- `move_overhead` de 2000 ms: do Brasil são ~200 ms por lance até os servidores do Lichess
  (OVH, Gravelines, França), e o Lichess **não compensa lag de bots**;
- só desafios **com incremento**, partidas padrão e Chess960, uma de cada vez;
- **só partidas casual e sem desafiar ninguém** na primeira fase.

## 3. Primeira fase: casual

Nas primeiras 50–100 partidas, acompanhe os PGNs em `C:\Projetos\lichess-bot\game_records` e o
log: nenhuma derrota por tempo, nenhum lance ilegal, nenhuma queda. Motivos para começar em casual:
- todo bot novo começa com rating **3000 provisório** e cai rápido nas primeiras partidas rated;
- a documentação da API pede casual enquanto o bot está em teste.

## 4. Segunda fase: rated e desafios automáticos

```powershell
venv\Scripts\python.exe C:\Projetos\ChessAI\scripts\lichess_config.py --engine C:\Projetos\ChessAI\target\release\caipora.exe --rated --matchmaking
```

O matchmaking desafia bots em 3+2 ou 5+3 depois de 1 minuto parado (era 10: com partidas de
~10 min, metade do dia ficava ocioso). Com 1 minuto o bot chega perto do teto do Lichess de 100
partidas entre bots por dia; partidas contra humanos não contam no teto. Duas partidas ao mesmo
tempo não compensam: com 2 vCPUs, cada uma jogaria com metade da máquina.

## Rede neural e faixa de adversários

```powershell
venv\Scripts\python.exe C:\Projetos\ChessAI\scripts\lichess_config.py `
  --engine C:\Projetos\lichess-bot\engines\<caipora>.exe `
  --eval-file C:\Projetos\lichess-bot\engines\<rede>.nnue --matchmaking
```

- `--eval-file` passa a rede como `EvalFile`; as duas partidas de fumaça já jogam com ela.
- `--opponent-rating MIN MAX` (padrão 2000 a 2600) limita os bots que o matchmaking desafia, em
  valores absolutos. Em partidas casual o rating do bot fica no provisório de 3000, e a
  diferença relativa do padrão (±300) só achava bots de 2750 para cima: das 10 primeiras
  partidas, 9 derrotas e 1 empate contra adversários bem mais fortes.
- `--rating-difference N` troca a faixa fixa por "rating atual do bot ± N", que o lichess-bot
  recalcula a cada desafio. Serve para achar onde o bot está depois que ele já joga rated; a faixa
  absoluta fica de reserva para quando ele não tem rating no ritmo. Desde 27/09/2026 o bot do
  servidor usa ±300 (com rating 2543 havia 59 bots online nessa faixa).
- `--rating-below N` estreita só o lado de baixo: com `--rating-difference 300 --rating-below 100`
  a janela é -100/+300. O lichess-bot não tem isso; vem do patch local
  `scripts/lichess-bot-rating-below.patch` (chave `opponent_rating_difference_below`). Desde
  01/10/2026 o bot usa -100/+300: o Lichess limita a 100 partidas bot contra bot a cada 24 h,
  somando todos os ritmos (contra humanos não há limite), então as vagas vão para adversários do
  nosso nível ou acima.

## Livro e tablebases (D19)

O bot joga as aberturas pelo chessdb e pela análise em nuvem do Lichess (melhor lance, profundidade
≥ 20; para de consultar depois de 10 posições sem lance) e finais de até 7 peças pela tablebase do
Lichess (com pelo menos 5 s no relógio). O log do lichess-bot mostra "Got move ... from chessdb.cn",
"... from lichess cloud analysis" e "... from lichess.org" (tablebase).

## Torneios de bots

- O bot está nas equipes **DarkOnBot** (`darkonbot`, a mais ativa: "FDG Open Bot and Humans" aos
  sábados 13h, "Bot League" aos domingos, team battles avulsos) e **Lichess Bots**.
- `scripts/tournament_join.py` (testes em `scripts/test_tournament_join.py`) roda de hora em hora
  no servidor pelo timer `caipora-tournaments` e inscreve o bot nas arenas das equipes dele que:
  aceitam bots, são de xadrez padrão e rated, duram de 3 a 50 minutos estimados (base + 40 ×
  incremento, como o Lichess classifica; sem bullet), ainda não
  começaram e começam em até 7 dias. Em team battle, joga por uma das equipes do bot na disputa.
- O Lichess limita as inscrições por período ("You are joining too many tournaments"): o script
  inscreve primeiro os torneios mais próximos e para ao bater no limite; a rodada seguinte continua.
- O bot pode estar em no máximo 15 equipes ("You have joined too many teams"); em 01/10/2026 saiu
  da Lichess Bots (sem torneios desde 2021) para entrar no The Sacrifice Club.
- As partidas do torneio chegam ao lichess-bot como qualquer outra; não precisa mexer no config.
- Token: o do bot, com `tournament:write` e `team:write` além de `bot:play`.
- Instalar ou atualizar:
  ```bash
  S=root@<servidor>
  scp scripts/tournament_join.py $S:/opt/caipora/
  scp deploy/caipora-tournaments.service deploy/caipora-tournaments.timer $S:/etc/systemd/system/
  ssh $S 'chown caipora:caipora /opt/caipora/tournament_join.py && systemctl daemon-reload     && systemctl enable --now caipora-tournaments.timer'
  ```
- Log: `/opt/caipora/tournaments.log`. Simular sem inscrever: rodar o script com `--dry-run`.
- Entrar em mais uma equipe: `POST /team/<id>/join` com o token do bot (equipes fechadas pedem
  aprovação do líder).

## Regras do Lichess que afetam o bot

- **100 partidas bot contra bot por dia**, contando desafios recebidos; contra humanos não há
  limite.
- Sem UltraBullet; sem pools nem Swiss; arenas só quando o organizador libera bots.
- Partida rated com os mesmos 20 primeiros meios-lances e o mesmo vencedor de uma das 2 anteriores
  contra o mesmo oponente **não conta** para o rating (detector de farming).
- O rating de bot no Lichess não mede força de engine acima de ~2500–2700 (comprime). A régua é
  SPRT local e, depois, a CCRL.

## Transparência (D1)

Coloque na bio da conta algo como: *"Caipora, UCI chess engine in Rust written by Claude Code
under the direction of Matheus de Carvalho Jesus."*

## Patch local no lichess-bot (partida repetida)

O Lichess às vezes manda o evento `gameStart` duas vezes para a mesma partida. O lichess-bot
2026.8.9.2 (e a versão oficial, em 27/09/2026) abre então dois processos para ela. O segundo
recebe HTTP 429 no stream da partida e pode derrubar o jogo: foi o que aconteceu em
<https://lichess.org/m7pWoX1F>, abandonada.

O patch `scripts/lichess-bot-duplicate-gamestart.patch` guarda os ids das partidas já
iniciadas, ignora o `gameStart` repetido e libera o id quando a partida acaba. Reaplicar depois de
cada atualização do lichess-bot:

```powershell
cd C:\Projetos\lichess-bot
git apply C:\Projetos\ChessAI\scripts\lichess-bot-duplicate-gamestart.patch
```

## Rodar 24 horas

- No PC: como serviço do Windows com o NSSM (o WSL não se mantém vivo sozinho). Desligue a
  suspensão e ajuste o horário de reinício do Windows Update.
- Mais tarde, numa VPS perto do Lichess (ver `docs/pesquisa-e-roteiro.md`, seção 8.3).

Para trocar a versão da engine: `quit_after_all_games_finish` está ligado, então Ctrl+C espera as
partidas terminarem; recompile e suba o bot de novo.
