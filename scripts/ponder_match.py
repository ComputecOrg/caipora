"""Partidas do Caipora com ponder contra ele mesmo sem ponder, conduzidas pelo python-chess.

O fastchess não pondera; o lichess-bot pondera pelo python-chess (`play(..., ponder=True)`: depois
do lance, `position` com a resposta esperada e `go ponder`; se a resposta vier, só `ponderhit`;
senão `stop` e um `go` novo). Este script usa exatamente esse caminho, então mede o ganho do ponder
e testa o protocolo como o bot o usa.

    python3 ponder_match.py <engine> <aberturas.epd> <pares> <processos> [tc base+inc] [saída.pgn]

Cada abertura é jogada duas vezes, trocando as cores. Cada partida usa até 2 núcleos (um pensa, o
outro pondera), então use processos <= metade dos núcleos. Placar do lado com ponder.
"""

import math
import multiprocessing
import random
import sys
import time

import chess
import chess.engine
import chess.pgn

MAX_PLIES = 400


def play_game(job):
    engine_path, fen, ponder_is_white, base, inc = job
    board = chess.Board(fen)
    ponder_side = chess.WHITE if ponder_is_white else chess.BLACK
    engines = {
        side: chess.engine.SimpleEngine.popen_uci(engine_path) for side in (chess.WHITE, chess.BLACK)
    }
    clocks = {chess.WHITE: base, chess.BLACK: base}
    stats = {"hits": 0, "predictions": 0, "with_ponder_move": 0, "moves": 0}
    game_id = object()
    expected = None  # resposta que o lado com ponder espera do adversário
    result, reason = None, None
    try:
        for side, engine in engines.items():
            engine.configure({"Hash": 16, "Threads": 1})
        while not board.is_game_over(claim_draw=True) and len(board.move_stack) < MAX_PLIES:
            side = board.turn
            limit = chess.engine.Limit(
                white_clock=clocks[chess.WHITE],
                black_clock=clocks[chess.BLACK],
                white_inc=inc,
                black_inc=inc,
            )
            start = time.perf_counter()
            played = engines[side].play(board, limit, ponder=side == ponder_side, game=game_id)
            clocks[side] -= time.perf_counter() - start
            if clocks[side] < 0:
                result = "0-1" if side == chess.WHITE else "1-0"
                reason = "tempo"
                break
            clocks[side] += inc
            if played.move is None or played.move not in board.legal_moves:
                result = "0-1" if side == chess.WHITE else "1-0"
                reason = f"lance ilegal {played.move}"
                break
            if side == ponder_side:
                stats["moves"] += 1
                if played.ponder is not None:
                    stats["with_ponder_move"] += 1
                expected = played.ponder
            elif expected is not None:
                stats["predictions"] += 1
                stats["hits"] += played.move == expected
                expected = None
            board.push(played.move)
    finally:
        for engine in engines.values():
            engine.quit()
    if result is None:
        outcome = board.outcome(claim_draw=True)
        result = outcome.result() if outcome else "1/2-1/2"
        reason = outcome.termination.name.lower() if outcome else "limite de lances"
    points = {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}[result]
    score = points if ponder_is_white else 1 - points
    game = chess.pgn.Game.from_board(board)
    game.headers["White"] = "ponder" if ponder_is_white else "sem-ponder"
    game.headers["Black"] = "sem-ponder" if ponder_is_white else "ponder"
    game.headers["Result"] = result
    game.headers["Termination"] = reason
    return score, reason, stats, str(game)


def elo(score):
    score = min(max(score, 1e-6), 1 - 1e-6)
    return -400 * math.log10(1 / score - 1)


def main():
    engine_path, book, pairs, processes = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
    base_text, inc_text = (sys.argv[5] if len(sys.argv) > 5 else "10+0.1").split("+")
    pgn_path = sys.argv[6] if len(sys.argv) > 6 else "ponder_match.pgn"
    base, inc = float(base_text), float(inc_text)
    with open(book, encoding="utf-8") as stream:
        openings = [line.split(";")[0].strip() for line in stream if line.strip()]
    random.Random(7).shuffle(openings)
    jobs = []
    for fen in openings[:pairs]:
        fen = fen if len(fen.split()) >= 6 else fen + " 0 1"
        jobs += [(engine_path, fen, True, base, inc), (engine_path, fen, False, base, inc)]
    scores, reasons = [], {}
    totals = {"hits": 0, "predictions": 0, "with_ponder_move": 0, "moves": 0}
    with multiprocessing.Pool(processes) as pool, open(pgn_path, "w", encoding="utf-8") as pgn:
        for done, (score, reason, stats, game) in enumerate(pool.imap_unordered(play_game, jobs), 1):
            scores.append(score)
            reasons[reason] = reasons.get(reason, 0) + 1
            for key in totals:
                totals[key] += stats[key]
            pgn.write(game + "\n\n")
            if done % 50 == 0 or done == len(jobs):
                print_summary(scores, reasons, totals)
    return 0


def print_summary(scores, reasons, totals):
    n = len(scores)
    mean = sum(scores) / n
    wins, draws, losses = (scores.count(1.0), scores.count(0.5), scores.count(0.0))
    variance = sum((s - mean) ** 2 for s in scores) / n
    margin = 1.96 * math.sqrt(variance / n) * 400 / math.log(10) / max(mean * (1 - mean), 1e-6)
    hit_rate = totals["hits"] / max(totals["predictions"], 1)
    ponder_rate = totals["with_ponder_move"] / max(totals["moves"], 1)
    print(
        f"{n} partidas: V{wins} E{draws} D{losses} ({100 * mean:.1f}%), "
        f"Elo {elo(mean):+.0f} ± {margin:.0f}; acerto do ponder {100 * hit_rate:.0f}%, "
        f"lances com ponder {100 * ponder_rate:.0f}%; terminações {reasons}",
        flush=True,
    )


if __name__ == "__main__":
    sys.exit(main())
