"""Gera o config.yml do lichess-bot para o Caipora, valida e faz um teste de fumaça.

Rodar DENTRO da pasta do lichess-bot, com o Python do venv dele:

    venv\\Scripts\\python.exe C:\\Projetos\\ChessAI\\scripts\\lichess_config.py \\
        --engine C:\\Projetos\\ChessAI\\target\\release\\caipora.exe

O token NÃO vai para o arquivo: o lichess-bot lê a variável de ambiente LICHESS_BOT_TOKEN.
Por padrão só aceita partidas casual e não desafia ninguém; use --rated e --matchmaking depois
das primeiras dezenas de partidas sem problema (ver docs/lichess.md).
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


def overrides(engine_path: str, rated: bool, matchmaking: bool) -> dict:
    return {
        "token": "",
        "engine": {
            "dir": os.path.dirname(os.path.abspath(engine_path)),
            "name": os.path.basename(engine_path),
            "protocol": "uci",
            # O Caipora ainda não implementa ponder.
            "ponder": False,
            "polyglot": {"enabled": False},
            # Nada de lances vindos de fora: o resultado tem de medir a nossa engine.
            "online_moves": {
                "chessdb_book": {"enabled": False},
                "lichess_cloud_analysis": {"enabled": False},
                "lichess_opening_explorer": {"enabled": False},
                "online_egtb": {"enabled": False},
            },
            "lichess_bot_tbs": {
                "syzygy": {"enabled": False},
                "gaviota": {"enabled": False},
            },
            "draw_or_resign": {"resign_enabled": False},
        },
        # Do Brasil são ~200 ms por lance até Gravelines, sem compensação de lag para bots.
        "move_overhead": 2000,
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
            "challenge_timeout": 10,
            "challenge_initial_time": [180, 300],
            "challenge_increment": [2, 3],
            "opponent_rating_difference": 250,
            "challenge_mode": "random" if rated else "casual",
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--engine", required=True, help="caminho do executável do Caipora")
    parser.add_argument("--rated", action="store_true", help="aceitar e pedir partidas rated")
    parser.add_argument("--matchmaking", action="store_true", help="desafiar outros bots")
    args = parser.parse_args()

    with open("config.yml.default", encoding="utf-8") as stream:
        config = yaml.safe_load(stream)
    merge(config, overrides(args.engine, args.rated, args.matchmaking))
    config["engine"]["uci_options"] = dict(UCI_OPTIONS)
    with open("config.yml", "w", encoding="utf-8") as stream:
        yaml.safe_dump(config, stream, sort_keys=False, allow_unicode=True)
    print("config.yml gerado")

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
