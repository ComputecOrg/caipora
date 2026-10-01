"""Testes do gerador de config do lichess-bot. Rodar com o Python do venv do lichess-bot:

    venv\\Scripts\\python.exe -m unittest discover -s C:\\Projetos\\ChessAI\\scripts
"""

import copy
import unittest

import lichess_config

# O essencial do config.yml.default do lichess-bot para estes testes.
DEFAULT = {
    "engine": {"uci_options": {"Threads": 4}},
    "challenge": {},
    "matchmaking": {"opponent_rating_difference": 300},
}


def build(*argv: str) -> dict:
    args = lichess_config.parse_args(["--engine", "/opt/caipora/caipora", *argv])
    return lichess_config.build_config(copy.deepcopy(DEFAULT), args)


class RatingWindow(unittest.TestCase):
    def test_the_default_is_an_absolute_range(self):
        matchmaking = build()["matchmaking"]
        self.assertNotIn("opponent_rating_difference", matchmaking)
        self.assertEqual(matchmaking["opponent_min_rating"], 2000)
        self.assertEqual(matchmaking["opponent_max_rating"], 2600)

    def test_a_rating_difference_follows_the_bot_rating(self):
        matchmaking = build("--rating-difference", "250")["matchmaking"]
        self.assertEqual(matchmaking["opponent_rating_difference"], 250)
        # A faixa absoluta fica de reserva: o lichess-bot só a usa enquanto o bot não tem rating.
        self.assertEqual(matchmaking["opponent_min_rating"], 2000)
        self.assertEqual(matchmaking["opponent_max_rating"], 2600)

    def test_the_window_below_can_be_narrower(self):
        # -100/+300: o patch local do lichess-bot lê o limite de baixo à parte.
        matchmaking = build("--rating-difference", "300", "--rating-below", "100")["matchmaking"]
        self.assertEqual(matchmaking["opponent_rating_difference"], 300)
        self.assertEqual(matchmaking["opponent_rating_difference_below"], 100)

    def test_without_a_window_below_the_window_is_symmetric(self):
        matchmaking = build("--rating-difference", "300")["matchmaking"]
        self.assertNotIn("opponent_rating_difference_below", matchmaking)


class Matchmaking(unittest.TestCase):
    def test_the_bot_challenges_again_after_one_idle_minute(self):
        self.assertEqual(build("--matchmaking")["matchmaking"]["challenge_timeout"], 1)


class OnlineMoves(unittest.TestCase):
    def test_openings_come_from_chessdb_and_the_lichess_cloud(self):
        online = build()["engine"]["online_moves"]
        self.assertTrue(online["chessdb_book"]["enabled"])
        self.assertEqual(online["chessdb_book"]["move_quality"], "best")
        self.assertTrue(online["lichess_cloud_analysis"]["enabled"])
        self.assertFalse(online["lichess_opening_explorer"]["enabled"])

    def test_endgames_up_to_seven_pieces_come_from_the_lichess_tablebase(self):
        egtb = build()["engine"]["online_moves"]["online_egtb"]
        self.assertTrue(egtb["enabled"])
        self.assertEqual((egtb["source"], egtb["max_pieces"], egtb["min_time"]), ("lichess", 7, 5))


class EngineOptions(unittest.TestCase):
    def test_the_bot_ponders(self):
        self.assertIs(build()["engine"]["ponder"], True)

    def test_uci_options_replace_the_defaults(self):
        self.assertEqual(
            build("--threads", "2")["engine"]["uci_options"],
            {"Hash": 128, "Move Overhead": 100, "Threads": 2},
        )


if __name__ == "__main__":
    unittest.main()
