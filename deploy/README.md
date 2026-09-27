# Bot do Lichess num servidor

O bot roda num servidor Linux como serviço do sistema (`caipora-bot`), com usuário próprio em
`/opt/caipora`, à parte do que já estiver na máquina. Primeiro servidor, em 27/09/2026: a VPS da
Hetzner do dono nos EUA (Ubuntu 24.04, 2 vCPU AMD EPYC com AVX2, 2 GB, Coolify instalado). A
latência até o Lichess é de 88 ms, contra ~200 ms de casa.

Comandos no Git Bash, na raiz do repositório. `S=root@<servidor>`.

## 1. Preparar a máquina (uma vez; pode repetir)

```bash
ssh $S 'bash -s' < deploy/server_setup.sh
scp scripts/lichess-bot-duplicate-gamestart.patch $S:/opt/caipora/
ssh $S 'chown caipora:caipora /opt/caipora/*.patch; bash -s' < deploy/server_setup.sh
```

O script instala o `python3.12-venv`, cria o usuário `caipora` e clona o lichess-bot na mesma versão
da máquina do dono. Na segunda vez, aplica o patch do `gameStart` repetido.

## 2. Engine (binário estático, sem depender da glibc do servidor)

No WSL:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

Depois:

```bash
scp target/x86_64-unknown-linux-musl/release/caipora $S:/opt/caipora/lichess-bot/engines/caipora-<commit>
ssh $S 'chown caipora:caipora /opt/caipora/lichess-bot/engines/*; chmod 755 /opt/caipora/lichess-bot/engines/*'
```

A rede vai dentro do executável (D14): não há arquivo `.nnue` para copiar.

## 3. Configuração do bot

```bash
scp scripts/lichess_config.py $S:/opt/caipora/
ssh $S 'cd /opt/caipora/lichess-bot && runuser -u caipora -- venv/bin/python /opt/caipora/lichess_config.py \
  --engine /opt/caipora/lichess-bot/engines/caipora-<commit> --matchmaking --move-overhead 1000'
```

O script gera o `config.yml`, valida com o carregador do próprio lichess-bot e joga duas partidas
de fumaça no servidor.

## 4. Token (o dono faz; o token não passa pelo chat nem pelo repositório)

1. Criar um token novo só com `bot:play`:
   <https://lichess.org/account/oauth/token/create?scopes[]=bot:play&description=caipora-server>.
2. Revogar o antigo em <https://lichess.org/account/oauth/token>.
3. Gravar o token no servidor, num terminal do próprio dono:

   ```bash
   ssh root@<servidor> 'umask 077; read -rsp "token: " t; echo "LICHESS_BOT_TOKEN=$t" > /etc/caipora-bot.env'
   ```

No PC de casa, atualizar a variável `LICHESS_BOT_TOKEN` se o bot ainda for rodar lá.

## 5. Serviço

```bash
scp deploy/caipora-bot.service $S:/etc/systemd/system/
scp deploy/caipora-bot.logrotate $S:/etc/logrotate.d/caipora-bot
ssh $S 'systemctl daemon-reload && systemctl enable --now caipora-bot'
```

- **Nunca rodar duas instâncias do bot com a mesma conta.** Parar o bot de casa antes de ligar
  o do servidor.
- Log: `ssh $S tail -f /opt/caipora/lichess-bot/bot.log`.
- Partidas: `/opt/caipora/lichess-bot/game_records`.
- Quedas da engine: `/opt/caipora/lichess-bot/engines/caipora-crash.log`.
- Trocar de versão: copiar o binário novo, gerar o config de novo (passo 3) e rodar
  `systemctl restart caipora-bot`. A parada espera as partidas em andamento terminarem.

# Máquinas temporárias na AWS (dados da rede e, depois, SPRTs)

Orçamento do dono: até ~R$ 100/mês. Na conta AWS há:

- **Orçamento `Caipora - computacao`:** US$ 18,62/mês, com e-mail a 50%, 80% e 100% do gasto real
  e na previsão de 100%.
- **Região eu-north-1 (Estocolmo):** foi a mais barata por núcleo em 27/09/2026. c7a.16xlarge
  (64 núcleos Zen 4, sem SMT) a US$ 0,367/h no spot, ~R$ 2/h.
- **Cota de spot:** 5 vCPUs de fábrica. Pedido de aumento para 192 enviado em 27/09/2026.
- **Chave SSH `caipora`:** `~/.ssh/caipora-aws.pem`, só no PC do dono. O `aws.exe` a salvou com
  CRLF, e foi preciso converter para LF (`tr -d '\r'`) e restringir com `icacls`.
- **Grupo `caipora-ssh`:** porta 22 aberta só para o IP do dono. Se o IP de casa mudar, atualizar
  a regra.

Login: `aws login --region eu-north-1` (credenciais temporárias, sem chave fixa no PC).

```bash
deploy/aws_spot.sh start c7a.16xlarge 3        # sobe; apaga-se sozinha em 3 h
deploy/aws_spot.sh datagen <id> tools/caipora-linux-<commit> nets/<rede>.nnue g3 20001 2
deploy/aws_spot.sh fetch <id> g3               # datagen/g3-aws-<id>.txt.gz
deploy/aws_spot.sh stop <id>
deploy/aws_spot.sh list                        # vazio = nada gerando custo
```

- **Travas contra máquina esquecida:** cada máquina agenda o próprio desligamento com
  "terminate", então desligar = apagar, e o disco vai junto.
- **Interrupção do spot:** a AWS pode tomar a máquina de volta a qualquer momento. Dados não
  trazidos com `fetch` se perdem; em rodadas longas, trazer de tempos em tempos.
- **Teste de 27/09/2026 (c7a.large, 2 núcleos, < US$ 0,01):**
  - subir, copiar o binário estático e a rede, gerar, trazer e apagar, sem sobrar disco;
  - ~240 posições/s por núcleo com a rede, 3 a 4× o ritmo por processo do PC de casa ocupado;
  - estimativa para a c7a.16xlarge: ~55 milhões de posições/h.
