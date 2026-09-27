#!/usr/bin/env bash
# Treina uma rede a partir dos textos do `caipora datagen`: fotografa as linhas completas (o datagen
# pode estar escrevendo), converte, embaralha, junta e treina na GPU. Passo a passo em docs/nnue.md.
#
# Do Git Bash:
#   MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -- bash /mnt/c/Projetos/ChessAI/scripts/wsl_train.sh \
#     <prefixo> <id> <épocas> [wdl] [lr]
# - <prefixo>: usa datagen/<prefixo>-*.txt e datagen/<prefixo>-*.txt.gz (trazidos da AWS).
# - Cada superbatch do treino é uma época (uma passada pelos dados).
# - A rede final vai para nets/<id>.nnue; os checkpoints ficam em ~/nnue/<id>/checkpoints.
# - As últimas 1% das linhas de cada arquivo (as partidas mais recentes) ficam fora do treino, em
#   nets/<id>-validation.txt, para `caipora validate` medir a perda em posições não vistas.
set -eu
PREFIX=$1; ID=$2; EPOCHS=$3; WDL=${4:-0.5}; LR=${5:-0.001}
REPO=/mnt/c/Projetos/ChessAI
UTILS=$HOME/bullet/target/release/bullet-utils
export PATH="$HOME/.cargo/bin:/usr/bin:/bin"
export CUDA_PATH="$HOME/cuda-shim"
export CARGO_TARGET_DIR="$HOME/caipora-trainer-target"

rm -rf "$HOME/caipora-trainer" && cp -r "$REPO/trainer" "$HOME/caipora-trainer"
(cd "$HOME/caipora-trainer" && rustup override set 1.98.1 >/dev/null && cargo build -r -q)

WORK=$HOME/nnue/$ID
mkdir -p "$WORK" && cd "$WORK"
rm -f part-*.bin part-*.txt validation.txt
for f in "$REPO"/datagen/"$PREFIX"-*.txt "$REPO"/datagen/"$PREFIX"-*.txt.gz; do
  [ -e "$f" ] || continue
  name=$(basename "$f"); name=${name%.gz}; name=${name%.txt}
  # Fotografia só com linhas completas (o datagen pode estar escrevendo; .gz vem da AWS).
  zcat -f "$f" > raw.txt
  lines=$(wc -l < raw.txt)
  held=$(( lines / 100 ))
  head -n "$lines" raw.txt > snapshot.txt
  rm raw.txt
  head -n $(( lines - held )) snapshot.txt > "part-$name.txt"
  tail -n "$held" snapshot.txt >> validation.txt
  rm snapshot.txt
  "$UTILS" convert --from text --input "part-$name.txt" --output "part-$name.bin" > /dev/null
  "$UTILS" shuffle --input "part-$name.bin" --output "part-$name-shuf.bin" --mem-used-mb 2048 > /dev/null
  rm "part-$name.txt" "part-$name.bin"
  echo "$name: $(( lines - held )) posições de treino, $held de validação"
done
"$UTILS" interleave part-*-shuf.bin --output data.bin > /dev/null
rm part-*-shuf.bin
POSITIONS=$(( $(stat -c %s data.bin) / 32 ))
BATCHES=$(( POSITIONS / 16384 ))
echo "total: $POSITIONS posições, $BATCHES lotes por época, $EPOCHS épocas, wdl $WDL, lr $LR"

"$CARGO_TARGET_DIR/release/caipora-trainer" data.bin "$ID" "$EPOCHS" "$BATCHES" "$WDL" "$LR" \
  2>&1 | sed -u 's/\x1b\[[0-9;]*m//g' | grep --line-buffered -E "running loss|Saved|Total Training"
mkdir -p "$REPO/nets"
cp "checkpoints/$ID-$EPOCHS/quantised.bin" "$REPO/nets/$ID.nnue"
cp validation.txt "$REPO/nets/$ID-validation.txt"
ls -la "$REPO/nets/$ID.nnue" "$REPO/nets/$ID-validation.txt"
echo "perda de validação: caipora validate nets/$ID.nnue nets/$ID-validation.txt $WDL"
