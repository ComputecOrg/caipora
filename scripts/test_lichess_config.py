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


class EngineOptions(unittest.TestCase):
    def test_uci_options_replace_the_defaults(self):
        self.assertEqual(
            build("--threads", "2")["engine"]["uci_options"],
            {"Hash": 128, "Move Overhead": 100, "Threads": 2},
        )


if __name__ == "__main__":
    unittest.main()
