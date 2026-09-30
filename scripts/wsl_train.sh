#!/usr/bin/env bash
# Treina uma rede a partir dos textos do `caipora datagen`: fotografa as linhas completas (o datagen
# pode estar escrevendo), converte, embaralha, junta e treina na GPU. Passo a passo em docs/nnue.md.
#
# Do Git Bash:
#   MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -- bash /mnt/c/Projetos/ChessAI/scripts/wsl_train.sh \
#     <prefixo> <id> <épocas> [wdl] [lr] [camada oculta, padrão 1024]
# - <prefixo>: usa datagen/<prefixo>-*.txt e datagen/<prefixo>-*.txt.gz (trazidos da AWS).
# - Cada superbatch do treino é uma época (uma passada pelos dados).
# - A rede final vai para nets/<id>.nnue; os checkpoints ficam em ~/nnue/<id>/checkpoints.
# - As últimas 1% das linhas de cada arquivo (as partidas mais recentes) ficam fora do treino, em
#   nets/<id>-validation.txt, para `caipora validate` medir a perda em posições não vistas.
set -eu
# O cache de disco da VM ocupa memória do Windows. Dentro de um escopo com limite de memória, o
# Linux recicla o cache em vez de crescer: sem ele, em 28/09/2026 a VM chegou a 6,1 GB
# descomprimindo um .gz de 2,6 GB e o Claude Code encerrou o treino por falta de memória. 3 GB
# cobrem o embaralhamento (2 GB) e o treino na GPU.
if [ -z "${CAIPORA_TRAIN_SCOPE:-}" ] && command -v systemd-run > /dev/null; then
  export CAIPORA_TRAIN_SCOPE=1
  exec systemd-run --scope --quiet -p MemoryMax=3G bash "$0" "$@"
fi
PREFIX=$1; ID=$2; EPOCHS=$3; WDL=${4:-0.5}; LR=${5:-0.001}
# A engine precisa ser compilada com o mesmo HIDDEN (src/nnue.rs) para usar a rede.
export CAIPORA_HIDDEN=${6:-1024}
REPO=/mnt/c/Projetos/ChessAI
UTILS=$HOME/bullet/target/release/bullet-utils
export PATH="$HOME/.cargo/bin:/usr/bin:/bin"
export CUDA_PATH="$HOME/cuda-shim"
export CARGO_TARGET_DIR="$HOME/caipora-trainer-target"

rm -rf "$HOME/caipora-trainer" && cp -r "$REPO/trainer" "$HOME/caipora-trainer"
(cd "$HOME/caipora-trainer" && rustup override set 1.98.1 >/dev/null && cargo build -r -q)

WORK=$HOME/nnue/$ID
mkdir -p "$WORK" && cd "$WORK"
# O cache de disco da VM ocupa memória do Windows até ser largado; com dezenas de GB de texto
# passando por aqui, o Windows fica sem memória e a leitura do /mnt/c falha ("Cannot allocate
# memory", medido em 27/09/2026 com um .gz de 2,6 GB). Larga o cache a cada arquivo.
release_cache() { sync; echo 1 > /proc/sys/vm/drop_caches 2>/dev/null || true; }

rm -f part-*.bin part-*.txt validation.txt raw.txt local.gz
release_cache
for f in "$REPO"/datagen/"$PREFIX"-*.txt "$REPO"/datagen/"$PREFIX"-*.txt.gz; do
  [ -e "$f" ] || continue
  name=$(basename "$f"); name=${name%.gz}; name=${name%.txt}
  # Fotografia do arquivo (o datagen pode estar escrevendo; .gz vem da AWS). O .gz é copiado
  # para o disco da VM antes de descomprimir, para ler pouco do /mnt/c.
  if [ "$f" != "${f%.gz}" ]; then
    cp "$f" local.gz
    zcat local.gz > raw.txt
    rm local.gz
  else
    cat "$f" > raw.txt
  fi
  # Só as linhas completas: a última pode estar pela metade.
  lines=$(wc -l < raw.txt)
  held=$(( lines / 100 ))
  head -n $(( lines - held )) raw.txt > "part-$name.txt"
  head -n "$lines" raw.txt | tail -n "$held" >> validation.txt
  rm raw.txt
  "$UTILS" convert --from text --input "part-$name.txt" --output "part-$name.bin" > /dev/null
  "$UTILS" shuffle --input "part-$name.bin" --output "part-$name-shuf.bin" --mem-used-mb 2048 > /dev/null
  rm "part-$name.txt" "part-$name.bin"
  release_cache
  echo "$name: $(( lines - held )) posições de treino, $held de validação"
done
shuffled=(part-*-shuf.bin)
if [ "${#shuffled[@]}" -eq 1 ]; then
  mv "${shuffled[0]}" data.bin   # o interleave exige pelo menos 2 arquivos
else
  "$UTILS" interleave "${shuffled[@]}" --output data.bin > /dev/null
  rm "${shuffled[@]}"
fi
POSITIONS=$(( $(stat -c %s data.bin) / 32 ))
BATCHES=$(( POSITIONS / 16384 ))
echo "total: $POSITIONS posições, $BATCHES lotes por época, $EPOCHS épocas, wdl $WDL, lr $LR, oculta $CAIPORA_HIDDEN"

"$CARGO_TARGET_DIR/release/caipora-trainer" data.bin "$ID" "$EPOCHS" "$BATCHES" "$WDL" "$LR" \
  2>&1 | sed -u 's/\x1b\[[0-9;]*m//g' | grep --line-buffered -E "camada oculta|running loss|Saved|Total Training"
mkdir -p "$REPO/nets"
cp "checkpoints/$ID-$EPOCHS/quantised.bin" "$REPO/nets/$ID.nnue"
cp validation.txt "$REPO/nets/$ID-validation.txt"
ls -la "$REPO/nets/$ID.nnue" "$REPO/nets/$ID-validation.txt"
echo "perda de validação: caipora validate nets/$ID.nnue nets/$ID-validation.txt $WDL"
