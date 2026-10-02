"""Driver de SPSA local para os parâmetros da busca do Caipora (só biblioteca padrão).

1. Compilar um executável com a feature `tune` (num diretório próprio, para não trocar o
   binário normal de target/release):

       cargo build --release --features tune --target-dir target/tune

   O resultado, target/tune/release/caipora.exe, expõe cada parâmetro de `src/tune.rs` como
   opção UCI `spin` com o mesmo nome, em minúsculas (rfp_margin, nmp_eval_div, lmr_divisor,
   lmr_deeper_base, capture_history_see_div, ...). O comando UCI `tune` lista todos no formato
   de entrada do SPSA do OpenBench (`nome, int, valor, min, max, c_end, r_end`):

       echo tune | target/tune/release/caipora.exe

   No fastchess cada um entra como `option.<nome>=<valor>` no `-engine`, por exemplo
   `-engine cmd=caipora.exe name=plus option.rfp_margin=84 option.nmp_eval_div=190`. O build
   normal (sem a feature) não tem essas opções e busca exatamente igual (mesmo bench).

2. Rodar a sessão (do diretório do repositório; caminhos relativos viram absolutos):

       python scripts/spsa.py --engine target/tune/release/caipora.exe \\
           --fastchess tools/fastchess/fastchess-windows-x86-64/fastchess.exe \\
           --book tools/8moves_v3.epd --iterations 2000 --pairs 8 --concurrency 5 \\
           --tc 8+0.08 --state spsa_state.json [--params rfp_margin,nmp_eval_div]

   A cada iteração o driver sorteia um sinal (+1/-1) por parâmetro, monta dois jogadores com o
   mesmo executável (valores + c·sinal e - c·sinal, passados como `option.<nome>=<valor>`),
   joga `--pairs` pares de partidas no fastchess entre eles e anda cada parâmetro na direção do
   lado que ganhou. Os passos seguem o esquema do SPSA do OpenBench: `c_end` é a perturbação na
   última iteração e `r_end` a taxa de aprendizado final (a = r_end·c_end²), com os expoentes
   clássicos 0,602 e 0,101 e A = 10% das iterações. `--concurrency` no máximo 5 na máquina de
   6 núcleos, e nada de outro teste pesado junto.

3. Estado e resultado: o estado vai para `--state` a cada iteração; rodar de novo com o mesmo
   arquivo retoma de onde parou (`--params` só vale na primeira vez). No fim (ou com `--report`)
   imprime os valores no formato do comando `tune` e uma linha `nome = valor` para levar de
   volta a `src/tune.rs`. Valor ajustado só entra na engine com SPRT contra a `main`.
"""

import argparse
import json
import os
import random
import re
import subprocess
import sys
from dataclasses import asdict, dataclass

ALPHA = 0.602
GAMMA = 0.101
RESULT = re.compile(r"Games: (\d+), Wins: (\d+), Losses: (\d+), Draws: (\d+)")


@dataclass
class Param:
    name: str
    value: float
    min: int
    max: int
    c_end: float
    r_end: float

    def clamp(self, x):
        return min(max(x, self.min), self.max)


def parse_tune_output(text):
    """Linhas `nome, int, valor, min, max, c_end, r_end` do comando UCI `tune`."""
    params = []
    for line in text.splitlines():
        fields = [f.strip() for f in line.split(",")]
        if len(fields) != 7 or fields[1] != "int":
            continue
        name, _, value, lo, hi, c_end, r_end = fields
        params.append(Param(name, float(value), int(lo), int(hi), float(c_end), float(r_end)))
    return params


def parse_result(text):
    """(vitórias, derrotas, empates) do primeiro jogador no último resumo do fastchess."""
    matches = RESULT.findall(text)
    if not matches:
        raise ValueError("saída do fastchess sem placar")
    _, wins, losses, draws = map(int, matches[-1])
    return wins, losses, draws


def schedule(param, iteration, total):
    """(c_k, r_k) do parâmetro na iteração `iteration` (1..total)."""
    big_a = 0.1 * total
    c = param.c_end * total**GAMMA
    a_end = param.r_end * param.c_end**2
    a = a_end * (big_a + total) ** ALPHA
    c_k = c / iteration**GAMMA
    a_k = a / (big_a + iteration) ** ALPHA
    return c_k, a_k / c_k**2


def perturb(params, iteration, total, rng):
    """Sinais sorteados e os valores inteiros dos jogadores + e -."""
    signs, plus, minus = [], [], []
    for p in params:
        s = rng.choice((-1, 1))
        c_k, _ = schedule(p, iteration, total)
        signs.append(s)
        plus.append(p.clamp(round(p.value + c_k * s)))
        minus.append(p.clamp(round(p.value - c_k * s)))
    return signs, plus, minus


def update(params, signs, wins, losses, iteration, total):
    """Anda cada parâmetro na direção do jogador + quando ele ganhou mais (e vice-versa)."""
    for p, s in zip(params, signs):
        c_k, r_k = schedule(p, iteration, total)
        p.value = p.clamp(p.value + r_k * c_k * (wins - losses) * s)


def fastchess_command(args, params, plus, minus, seed):
    """Linha de comando do fastchess: o mesmo executável dos dois lados, cada um com os seus
    valores como opções UCI (`option.<nome>=<valor>`)."""

    def engine(name, values):
        options = [f"option.{p.name}={v}" for p, v in zip(params, values)]
        return ["-engine", f"cmd={args.engine}", f"name={name}", *options]

    return [
        args.fastchess,
        *engine("plus", plus),
        *engine("minus", minus),
        "-each", f"tc={args.tc}", f"option.Hash={args.hash}",
        "-openings", f"file={args.book}", "format=epd", "order=random",
        "-srand", str(seed),
        "-rounds", str(args.pairs), "-games", "2", "-repeat",
        "-concurrency", str(args.concurrency), "-recover",
    ]


def play(args, params, plus, minus, seed):
    command = fastchess_command(args, params, plus, minus, seed)
    run = subprocess.run(command, capture_output=True, text=True, check=False)
    return parse_result(run.stdout)


def engine_parameters(engine):
    run = subprocess.run(
        [engine], input="tune\nquit\n", capture_output=True, text=True, check=True
    )
    params = parse_tune_output(run.stdout)
    if not params:
        sys.exit(f"{engine} não listou parâmetros: foi compilado com --features tune?")
    return params


def report(params):
    for p in params:
        print(f"{p.name}, int, {round(p.value)}, {p.min}, {p.max}, {p.c_end:g}, {p.r_end:g}")
    print()
    for p in params:
        print(f"{p.name} = {round(p.value)}  ({p.value:.2f})")


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--engine", required=True, help="executável compilado com --features tune")
    parser.add_argument("--fastchess", required=True, help="executável do fastchess")
    parser.add_argument("--book", required=True, help="aberturas .epd")
    parser.add_argument("--iterations", type=int, default=1000)
    parser.add_argument("--pairs", type=int, default=8, help="pares de partidas por iteração")
    parser.add_argument("--concurrency", type=int, default=5)
    parser.add_argument("--tc", default="8+0.08")
    parser.add_argument("--hash", type=int, default=16)
    parser.add_argument("--params", help="só estes parâmetros, separados por vírgula")
    parser.add_argument("--state", default="spsa_state.json")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--report", action="store_true", help="só imprime o estado salvo")
    return parser.parse_args(argv)


def main():
    # Acentos na ajuda e no relatório mesmo com a saída redirecionada (no Windows o padrão de
    # pipe é cp1252).
    sys.stdout.reconfigure(encoding="utf-8")
    args = parse_args()
    # Caminhos absolutos: no Windows o CreateProcess não acha executável relativo com "/".
    args.engine, args.fastchess, args.book = map(
        os.path.abspath, (args.engine, args.fastchess, args.book)
    )

    if os.path.exists(args.state):
        with open(args.state, encoding="utf-8") as f:
            state = json.load(f)
        params = [Param(**p) for p in state["params"]]
        done = state["iteration"]
    else:
        params = engine_parameters(args.engine)
        if args.params:
            wanted = set(args.params.split(","))
            params = [p for p in params if p.name in wanted]
            missing = wanted - {p.name for p in params}
            if missing:
                sys.exit(f"parâmetros desconhecidos: {', '.join(sorted(missing))}")
        done = 0
    if args.report:
        report(params)
        return

    total = args.iterations
    for iteration in range(done + 1, total + 1):
        rng = random.Random(args.seed * 1_000_003 + iteration)
        signs, plus, minus = perturb(params, iteration, total, rng)
        wins, losses, draws = play(args, params, plus, minus, rng.randrange(1 << 31))
        update(params, signs, wins, losses, iteration, total)
        with open(args.state + ".tmp", "w", encoding="utf-8") as f:
            json.dump({"iteration": iteration, "params": [asdict(p) for p in params]}, f, indent=1)
        os.replace(args.state + ".tmp", args.state)
        moved = ", ".join(f"{p.name}={p.value:.1f}" for p in params[:6])
        more = " ..." if len(params) > 6 else ""
        print(f"{iteration}/{total} +{wins} -{losses} ={draws}  {moved}{more}", flush=True)
    report(params)


if __name__ == "__main__":
    main()
