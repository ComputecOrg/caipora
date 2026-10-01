#!/usr/bin/env bash
# Prepara um servidor Linux (Ubuntu 24.04) para rodar o bot do Lichess como serviço, à parte do
# que já existir na máquina. Idempotente: pode rodar de novo sem estragar nada.
#
# Do Git Bash, na raiz do repositório:
#   ssh root@<servidor> 'bash -s' < deploy/server_setup.sh
#   scp scripts/lichess-bot-*.patch root@<servidor>:/opt/caipora/
#   ssh root@<servidor> 'bash -s' < deploy/server_setup.sh     (segunda vez: aplica o patch)
#
# O que faz:
# - instala o python3.12-venv;
# - cria o usuário de sistema `caipora` (home /opt/caipora, sem shell);
# - clona o lichess-bot na mesma versão da máquina do dono e cria o venv;
# - aplica os patches locais (gameStart repetido; janela de rating -N/+M), se os arquivos estiverem lá
#   (docs/lichess.md).
# A engine, a rede, o config.yml, o token e o serviço vêm depois (deploy/README.md).
set -euo pipefail
LICHESS_BOT_COMMIT=df7e730de58cc3ef2f1415a0dc2eeda842d39167   # 2026.8.9.2
HOME_DIR=/opt/caipora
BOT_DIR=$HOME_DIR/lichess-bot

if ! dpkg -s python3.12-venv >/dev/null 2>&1; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq python3.12-venv >/dev/null
fi
id caipora >/dev/null 2>&1 || useradd --system --create-home --home-dir "$HOME_DIR" \
  --shell /usr/sbin/nologin caipora
as_caipora() { runuser -u caipora -- "$@"; }

if [ ! -d "$BOT_DIR/.git" ]; then
  as_caipora git clone -q https://github.com/lichess-bot-devs/lichess-bot "$BOT_DIR"
fi
as_caipora git -C "$BOT_DIR" fetch -q origin
as_caipora git -C "$BOT_DIR" checkout -q "$LICHESS_BOT_COMMIT"
[ -x "$BOT_DIR/venv/bin/python" ] || as_caipora python3 -m venv "$BOT_DIR/venv"
as_caipora "$BOT_DIR/venv/bin/pip" install -q -r "$BOT_DIR/requirements.txt"

# Patches locais: arquivo, marca que prova que já está aplicado e arquivo onde procurar a marca.
apply_patch() {
  local patch=$HOME_DIR/$1 mark=$2 file=$BOT_DIR/$3
  [ -f "$patch" ] || return 0
  if as_caipora git -C "$BOT_DIR" apply --check "$patch" 2>/dev/null; then
    as_caipora git -C "$BOT_DIR" apply "$patch"
    echo "patch $1 aplicado"
  elif grep -q "$mark" "$file"; then
    echo "patch $1 já estava aplicado"
  else
    echo "o patch $1 não se aplica a esta versão do lichess-bot" >&2
    exit 1
  fi
}
apply_patch lichess-bot-duplicate-gamestart.patch started_games lib/lichess_bot.py
apply_patch lichess-bot-rating-below.patch opponent_rating_difference_below lib/matchmaking.py
mkdir -p "$BOT_DIR/engines"
chown caipora:caipora "$BOT_DIR/engines"
echo "lichess-bot $(as_caipora git -C "$BOT_DIR" describe --always) pronto em $BOT_DIR"
