#!/usr/bin/env bash
# Treina uma rede a partir dos textos do `caipora datagen`: fotografa as linhas completas (o datagen
# pode estar escrevendo), converte, embaralha, junta e treina na GPU. Passo a passo em docs/nnue.md.
#
# Do Git Bash:
#   MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -- bash /mnt/c/Projetos/ChessAI/scripts/wsl_train.sh \
#     <prefixo> <id> <épocas> [wdl] [lr]
# - <prefixo>: usa datagen/<prefixo>-*.txt.
# - Cada superbatch do treino é uma época (uma passada pelos dados).
# - A rede final vai para nets/<id>.nnue; os checkpoints ficam em ~/nnue/<id>/checkpoints.
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
rm -f part-*.bin part-*.txt
for f in "$REPO"/datagen/"$PREFIX"-*.txt; do
  name=$(basename "$f" .txt)
  lines=$(wc -l < "$f")
  head -n "$lines" "$f" > "part-$name.txt"
  "$UTILS" convert --from text --input "part-$name.txt" --output "part-$name.bin" > /dev/null
  "$UTILS" shuffle --input "part-$name.bin" --output "part-$name-shuf.bin" --mem-used-mb 2048 > /dev/null
  rm "part-$name.txt" "part-$name.bin"
  echo "$name: $lines posições"
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
ls -la "$REPO/nets/$ID.nnue"
