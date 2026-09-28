# NNUE: dos dados à rede na engine

Tudo com dados do próprio Caipora (D2). A arquitetura é a mesma em três lugares, que precisam
bater: `src/nnue.rs` (inferência), `trainer/src/main.rs` (treino) e o arquivo da rede.

- Arquitetura: (768 → 256)×2 → 1, SCReLU.
- Quantização: QA = 255, QB = 64, escala 400.
- Arquivo: 394.816 bytes.

## 1. Gerar dados (Windows, um processo por núcleo livre)

```bash
mkdir -p datagen
for seed in 1 2 3 4 5; do
  ./target/release/caipora.exe datagen 100000 datagen/g1-$seed.txt $seed 5000 2> datagen/g1-$seed.log &
done
```

- Cada linha: `<FEN> | <pontuação> | <resultado>`, do ponto de vista das brancas.
- A mesma semente gera os mesmos dados; sementes diferentes, partidas diferentes.
- Ritmo medido: ~190 posições/s por núcleo, com 5000 nós por lance.

## 2. Preparar o treino (WSL, uma vez)

- CUDA: `sudo apt-get install nvidia-cuda-toolkit` (12.4 no Ubuntu 26.04). O pacote não tem a
  pasta `lib64` que o `bullet` procura, então `CUDA_PATH` aponta para `~/cuda-shim`, com três
  links:
  - `include` → `/usr/include`;
  - `lib64` → `/usr/lib/x86_64-linux-gnu`;
  - `bin` → `/usr/bin`.
- `bullet` clonado em `~/bullet`, com `cargo build -r --package bullet-utils`.
- O treinador:

  ```bash
  cp -r /mnt/c/Projetos/ChessAI/trainer ~/caipora-trainer
  cd ~/caipora-trainer
  CUDA_PATH=~/cuda-shim cargo build -r
  ```

## Atalho: um comando do texto à rede

```bash
MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -- bash /mnt/c/Projetos/ChessAI/scripts/wsl_train.sh   <prefixo> <id> <épocas> [wdl] [lr]
```

O script:
- fotografa as linhas completas de `datagen/<prefixo>-*.txt` (o datagen pode estar escrevendo);
- converte, embaralha e junta os arquivos;
- treina com uma época por superbatch;
- grava a rede em `nets/<id>.nnue`.

Os passos 3 e 4 abaixo são o que ele faz.

Medido em 27/09/2026: 2,5 milhões de posições, 10 épocas em 7 s (~4,7 milhões de posições/s na
GTX 1660).

## 3. Converter, embaralhar e juntar

```bash
U=~/bullet/target/release/bullet-utils
for f in /mnt/c/Projetos/ChessAI/datagen/g1-*.txt; do
  b=$(basename "$f" .txt)
  $U convert --from text --input "$f" --output "$b.bin"
  $U shuffle --input "$b.bin" --output "$b-shuf.bin" --mem-used-mb 4096
done
$U interleave g1-*-shuf.bin --output g1.bin
```

## 4. Treinar (GTX 1660)

```bash
caipora-trainer g1.bin caipora-g1 <superbatches> [lotes por superbatch] [wdl] [lr]
```

- Um lote tem 16.384 posições; o padrão é 6104 lotes por superbatch (~100 milhões de posições).
- `wdl` é o peso do resultado da partida no alvo (padrão 0,5); o resto é a pontuação da busca.
- A taxa de aprendizado cai em cosseno até 1% da inicial.
- A rede sai em `checkpoints/<id>-<superbatch>/quantised.bin`.

## Rede embutida

`net/caipora-g2.nnue` vai dentro do executável (D14, D16) e é a avaliação padrão. Pela opção
`EvalFile`:

| Valor | Avaliação |
|---|---|
| `<embedded>` (padrão) | a rede do executável |
| `none` | a avaliação à mão |
| um caminho | outra rede, para testar |

Trocar a rede embutida muda o bench: o commit precisa do novo `Bench:`.

## 5. Testar na engine

```
setoption name EvalFile value <caminho>/quantised.bin
```

Força só se mede com SPRT contra a engine sem rede (ou com a rede anterior):
`option.EvalFile=...` só no lado novo, no `fastchess`.
