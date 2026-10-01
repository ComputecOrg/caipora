"""Gera o config.yml do lichess-bot para o Caipora, valida e faz um teste de fumaça.

Rodar DENTRO da pasta do lichess-bot, com o Python do venv dele:

    venv\\Scripts\\python.exe C:\\Projetos\\ChessAI\\scripts\\lichess_config.py \\
        --engine C:\\Projetos\\ChessAI\\target\\release\\caipora.exe

O token NÃO vai para o arquivo: o lichess-bot lê a variável de ambiente LICHESS_BOT_TOKEN.
Por padrão só aceita partidas casual e não desafia ninguém; use --rated e --matchmaking depois
das primeiras dezenas de partidas sem problema (ver docs/lichess.md).

--eval-file passa a rede neural ao Caipora (opção UCI EvalFile). --opponent-rating MIN MAX limita
os bots que o matchmaking desafia (padrão 2000 a 2600): em partidas casual o rating do bot fica no
provisório de 3000, e a diferença relativa só acharia bots bem mais fortes. Com o bot já rated,
--rating-difference N desafia bots a até N pontos do rating atual dele, que o lichess-bot recalcula
a cada desafio; a faixa absoluta fica de reserva para quando ele ainda não tem rating.
"""

import argparse
import os
import sys
import time

import chess
import chess.engine
import yaml


# Substitui (não mescla) as opções UCI do padrão: ele traz Threads 4, SyzygyPath e UCI_ShowWDL,
# e opção que o Caipora não declara, ou fora da faixa, derruba a engine logo no início da partida.
UCI_OPTIONS = {"Hash": 128, "Move Overhead": 100}


def merge(base: dict, overrides: dict) -> None:
    for key, value in overrides.items():
        if isinstance(value, dict) and isinstance(base.get(key), dict):
            merge(base[key], value)
        else:
            base[key] = value


def overrides(
    engine_path: str,
    rated: bool,
    matchmaking: bool,
    rating_range: tuple[int, int],
    move_overhead: int,
) -> dict:
    return {
        "token": "",
        "engine": {
            "dir": os.path.dirname(os.path.abspath(engine_path)),
            "name": os.path.basename(engine_path),
            "protocol": "uci",
            # Pensa no tempo do adversário: `go ponder` com a resposta esperada; no acerto, o
            # tempo pensado conta como gasto e, se já cobriu o orçamento, o lance sai na hora.
            "ponder": True,
            "polyglot": {"enabled": False},
            # Aberturas e finais vêm de fora (D19, decisão do dono pelos 3000 no Lichess): lances
            # de livro saem na hora e poupam o relógio; finais de até 7 peças saem perfeitos. As
            # listas (CCRL, gauntlet) continuam medindo só a engine, com os livros delas.
            "online_moves": {
                "max_out_of_book_moves": 10,
                "chessdb_book": {"enabled": True, "min_time": 20, "move_quality": "best"},
                "lichess_cloud_analysis": {"enabled": True, "min_time": 20, "move_quality": "best"},
                # Escolhas de humanos, não o melhor lance.
                "lichess_opening_explorer": {"enabled": False},
                "online_egtb": {
                    "enabled": True,
                    "min_time": 5,
                    "max_pieces": 7,
                    "source": "lichess",
                    "move_quality": "best",
                },
            },
            "lichess_bot_tbs": {
                "syzygy": {"enabled": False},
                "gaviota": {"enabled": False},
            },
            "draw_or_resign": {"resign_enabled": False},
        },
        # Folga por lance para a latência até o Lichess, que não compensa lag de bots: ~200 ms do
        # Brasil (padrão 2000), ~90 ms do servidor nos EUA.
        "move_overhead": move_overhead,
        "quit_after_all_games_finish": True,
        "pgn_directory": "game_records",
        "challenge": {
            "concurrency": 1,
            "min_increment": 1,
            "bullet_requires_increment": True,
            "variants": ["standard", "chess960"],
            "time_controls": ["bullet", "blitz", "rapid", "classical"],
            "modes": ["casual", "rated"] if rated else ["casual"],
            "max_simultaneous_games_per_user": 1,
        },
        "matchmaking": {
            "allow_matchmaking": matchmaking,
            "challenge_variant": "standard",
            # Minutos parado antes de desafiar de novo (o mínimo é 1). Com 10, metade do dia ficava
            # ocioso; com 1, o bot chega perto do teto do Lichess de 100 partidas entre bots por dia.
            "challenge_timeout": 1,
            "challenge_initial_time": [180, 300],
            "challenge_increment": [2, 3],
            # Limites absolutos; com --rating-difference, só valem enquanto o bot não tem rating
            # (ver build_config).
            "opponent_min_rating": rating_range[0],
            "opponent_max_rating": rating_range[1],
            # Com --rated, os desafios que o bot envia valem rating; os recebidos podem ser dos dois.
            "challenge_mode": "rated" if rated else "casual",
            "challenge_filter": "fine",
        },
    }


def smoke_game(engine_path: str, uci_options: dict, chess960: bool) -> str:
    """Partida curta da engine contra ela mesma, como o lichess-bot a conduz."""
    board = chess.Board.from_chess960_pos(314) if chess960 else chess.Board()
    engine = chess.engine.SimpleEngine.popen_uci(engine_path)
    increment = 0.1
    try:
        engine.configure(uci_options)
        # Relógio de verdade: desconta o tempo gasto e soma o incremento a cada lance.
        clock = {chess.WHITE: 10.0, chess.BLACK: 10.0}
        ply = 0
        while not board.is_game_over(claim_draw=True) and ply < 60:
            if ply < 2:
                # O lichess-bot usa tempo fixo no primeiro lance de cada lado.
                limit = chess.engine.Limit(time=1.0)
            else:
                limit = chess.engine.Limit(
                    white_clock=clock[chess.WHITE],
                    black_clock=clock[chess.BLACK],
                    white_inc=increment,
                    black_inc=increment,
                )
            side = board.turn
            start = time.monotonic()
            result = engine.play(board, limit)
            if ply >= 2:
                clock[side] += increment - (time.monotonic() - start)
                if clock[side] <= 0:
                    raise RuntimeError(f"estourou o tempo no meio-lance {ply}")
            if result.move not in board.legal_moves:
                raise RuntimeError(f"lance ilegal {result.move} em {board.fen()}")
            board.push(result.move)
            ply += 1
        variant = "Chess960" if chess960 else "padrão"
        return (
            f"{variant}: {ply} meios-lances sem erro; relógio final "
            f"{clock[chess.WHITE]:.1f}s x {clock[chess.BLACK]:.1f}s"
        )
    finally:
        engine.quit()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--engine", required=True, help="caminho do executável do Caipora")
    parser.add_argument("--rated", action="store_true", help="aceitar e pedir partidas rated")
    parser.add_argument("--matchmaking", action="store_true", help="desafiar outros bots")
    parser.add_argument("--eval-file", help="rede neural (.nnue) passada como EvalFile")
    parser.add_argument(
        "--threads", type=int, default=1, help="threads da busca (padrão 1; o servidor tem 2)"
    )
    parser.add_argument(
        "--opponent-rating",
        nargs=2,
        type=int,
        default=[2000, 2600],
        metavar=("MIN", "MAX"),
        help="faixa de rating dos bots desafiados",
    )
    parser.add_argument(
        "--rating-difference",
        type=int,
        metavar="N",
        help="desafiar bots a até N pontos do rating atual do bot (em vez da faixa fixa)",
    )
    parser.add_argument(
        "--rating-below",
        type=int,
        metavar="N",
        help="com --rating-difference, limite só para baixo (ex.: 100 para -100/+300); "
        "precisa do patch local scripts/lichess-bot-rating-below.patch",
    )
    parser.add_argument(
        "--move-overhead",
        type=int,
        default=2000,
        help="folga por lance em ms (padrão 2000, pensado para ~200 ms de latência)",
    )
    return parser.parse_args(argv)


def build_config(config: dict, args: argparse.Namespace) -> dict:
    """Aplica as escolhas do Caipora sobre o config.yml.default do lichess-bot."""
    rating_range = (args.opponent_rating[0], args.opponent_rating[1])
    merge(
        config,
        overrides(args.engine, args.rated, args.matchmaking, rating_range, args.move_overhead),
    )
    if args.rating_difference is None:
        # Sem a chave o lichess-bot usa os limites absolutos; com valor vazio o validador falha.
        config["matchmaking"].pop("opponent_rating_difference", None)
    else:
        config["matchmaking"]["opponent_rating_difference"] = args.rating_difference
        if args.rating_below is not None:
            config["matchmaking"]["opponent_rating_difference_below"] = args.rating_below
    config["engine"]["uci_options"] = dict(UCI_OPTIONS)
    if args.threads > 1:
        config["engine"]["uci_options"]["Threads"] = args.threads
    return config


def main() -> int:
    args = parse_args()
    with open("config.yml.default", encoding="utf-8") as stream:
        config = build_config(yaml.safe_load(stream), args)
    if args.eval_file:
        eval_file = os.path.abspath(args.eval_file)
        if not os.path.isfile(eval_file):
            print(f"rede não encontrada: {eval_file}")
            return 1
        config["engine"]["uci_options"]["EvalFile"] = eval_file
    with open("config.yml", "w", encoding="utf-8") as stream:
        yaml.safe_dump(config, stream, sort_keys=False, allow_unicode=True)
    print("config.yml gerado")
    print(f"opções UCI: {config['engine']['uci_options']}")
    if args.rating_difference is None:
        opponents = f"bots de {args.opponent_rating[0]} a {args.opponent_rating[1]}"
    else:
        below = args.rating_difference if args.rating_below is None else args.rating_below
        opponents = f"bots de -{below} a +{args.rating_difference} do rating do bot"
    print(f"matchmaking: {'ligado' if args.matchmaking else 'desligado'}, {opponents}, "
          f"{'rated' if args.rated else 'casual'}, folga {args.move_overhead} ms")

    # Validação com o próprio carregador do lichess-bot (não acessa a rede).
    sys.path.insert(0, os.getcwd())
    from lib.config import load_config

    os.environ.setdefault("LICHESS_BOT_TOKEN", "validacao-offline")
    load_config("config.yml")
    print("config.yml válido para o lichess-bot")

    for chess960 in (False, True):
        print(smoke_game(args.engine, config["engine"]["uci_options"], chess960))
    return 0


if __name__ == "__main__":
    sys.exit(main())
