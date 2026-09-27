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
