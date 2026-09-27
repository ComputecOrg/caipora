#!/usr/bin/env bash
# Régua de força: compila o Caipora no Linux (WSL), confere o bench e joga um gauntlet contra
# versões do Stash com rating CCRL Blitz conhecido (a escada que a comunidade usa).
#
# Rodar do Windows, no Git Bash:
#   MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -- bash /mnt/c/Projetos/ChessAI/scripts/wsl_gauntlet.sh
# Variáveis: BRANCH (padrão main), ROUNDS (pares de partidas por adversário, padrão 30),
# TC (padrão 8+0.08), STASH (versões, padrão "v12 v15.3 v19.0 v21.0").
#
# Ratings CCRL Blitz das versões do Stash (26/09/2026): v12 1883, v15.3 2173, v17 2297,
# v19 2473, v20.0.1 2511, v21 2713, v25 2933, v30 3153, v33 3273, v37 3419.
set -u
BRANCH=${BRANCH:-main}
ROUNDS=${ROUNDS:-30}
TC=${TC:-8+0.08}
STASH=${STASH:-"v12 v15.3 v19.0 v21.0"}
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v rustup >/dev/null; then
  curl -sSf https://sh.rustup.rs -o /tmp/rustup.sh
  sh /tmp/rustup.sh -y --profile minimal --default-toolchain none >/tmp/rustup.log 2>&1
fi
mkdir -p ~/gauntlet && cd ~/gauntlet || exit 1

# Stash: código GPL-3.0, usado só como adversário; compilado a partir das tags.
[ -d stash-src ] || git clone -q https://github.com/mhouppin/stash-bot stash-src
for tag in $STASH; do
  [ -x "stash-$tag" ] && continue
  git -C stash-src checkout -q -f "$tag"
  mapfile -t srcs < <(find stash-src -name '*.c' -not -path '*test*' -not -path '*tuner*' -not -path '*tools*')
  incs=$(find stash-src -type d -name include -printf '-I%p ')
  gcc -O3 -DNDEBUG -flto -march=x86-64-v2 "${srcs[@]}" $incs -o "stash-$tag" -lm -lpthread \
    || { echo "falhou ao compilar o Stash $tag"; exit 1; }
done

rm -rf caipora && git clone -q -b "$BRANCH" /mnt/c/Projetos/ChessAI caipora
(cd caipora && cargo build --release -q) || exit 1
echo "commit: $(git -C caipora log --oneline -1)"
echo "bench linux: $(./caipora/target/release/caipora bench | tail -1)"

if [ ! -x fastchess/fastchess ]; then
  mkdir -p fastchess
  curl -sSL https://github.com/Disservin/fastchess/releases/download/v1.8.2-alpha/fastchess-linux-x86-64.tar -o fastchess.tar
  tar -xf fastchess.tar -C fastchess
  cp "$(find fastchess -type f -name fastchess | head -1)" fastchess/fastchess 2>/dev/null
  chmod +x fastchess/fastchess
fi
if [ ! -f 8moves_v3.epd ]; then
  curl -sSL https://raw.githubusercontent.com/AndyGrant/openbench-books/master/8moves_v3.epd.zip -o book.zip
  python3 -c "import zipfile; zipfile.ZipFile('book.zip').extractall('.')"
fi

rm -f gauntlet.pgn gauntlet.log config.json
opponents=()
for tag in $STASH; do opponents+=(-engine "cmd=./stash-$tag" "name=Stash-$tag"); done
./fastchess/fastchess \
  -engine cmd=./caipora/target/release/caipora name=Caipora "${opponents[@]}" \
  -tournament gauntlet -seeds 1 \
  -each "tc=$TC" option.Hash=16 \
  -openings file=8moves_v3.epd format=epd order=random -srand 7 \
  -rounds "$ROUNDS" -games 2 -repeat -concurrency 5 \
  -pgnout file=gauntlet.pgn -log file=gauntlet.log level=warn 2>&1 | grep -vE '^(Started|Finished) game|Warning'
echo "--- terminações"
grep -h '^\[Termination' gauntlet.pgn | sort | uniq -c
