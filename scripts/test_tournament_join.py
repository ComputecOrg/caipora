"""Testes da inscrição automática em torneios. Rodar com qualquer Python 3:

    python -m unittest discover -s C:\\Projetos\\ChessAI\\scripts -p "test_tournament*.py"
"""

import unittest

import tournament_join

NOW = 1_800_000_000_000  # ms


def arena(**overrides) -> dict:
    """Arena como `GET /api/tournament/{id}` devolve (só os campos usados)."""
    t = {
        "id": "abcd1234",
        "fullName": "Bot Blitz Arena",
        "status": 10,
        "startsAt": NOW + 3_600_000,
        "clock": {"limit": 180, "increment": 2},
        "variant": "standard",
        "rated": True,
        "botsAllowed": True,
    }
    t.update(overrides)
    return t


class Choice(unittest.TestCase):
    def test_a_rated_standard_blitz_arena_open_to_bots_is_joined(self):
        self.assertIsNone(tournament_join.reason_to_skip(arena(), NOW))

    def test_arenas_closed_to_bots_are_skipped(self):
        self.assertEqual(tournament_join.reason_to_skip(arena(botsAllowed=False), NOW), "sem bots")
        self.assertEqual(tournament_join.reason_to_skip(arena(botsAllowed=None), NOW), "sem bots")

    def test_variants_are_skipped_whatever_the_api_shape(self):
        self.assertEqual(tournament_join.reason_to_skip(arena(variant="atomic"), NOW), "variante")
        self.assertIsNone(tournament_join.reason_to_skip(arena(variant={"key": "standard"}), NOW))
        self.assertEqual(
            tournament_join.reason_to_skip(arena(variant={"key": "chess960"}), NOW), "variante"
        )

    def test_casual_arenas_are_skipped(self):
        self.assertEqual(tournament_join.reason_to_skip(arena(rated=False), NOW), "casual")

    def test_only_blitz_and_rapid_clocks(self):
        # Bullet (base abaixo de 3 min) e partidas longas demais ficam de fora.
        bullet = arena(clock={"limit": 60, "increment": 0})
        self.assertEqual(tournament_join.reason_to_skip(bullet, NOW), "ritmo")
        two_one = arena(clock={"limit": 120, "increment": 1})
        self.assertEqual(tournament_join.reason_to_skip(two_one, NOW), "ritmo")
        classical = arena(clock={"limit": 1800, "increment": 0})
        self.assertEqual(tournament_join.reason_to_skip(classical, NOW), "ritmo")
        self.assertIsNone(tournament_join.reason_to_skip(arena(clock={"limit": 600, "increment": 5}), NOW))

    def test_started_finished_or_far_away_arenas_are_skipped(self):
        self.assertEqual(tournament_join.reason_to_skip(arena(status=20), NOW), "já começou")
        self.assertEqual(tournament_join.reason_to_skip(arena(isFinished=True), NOW), "já começou")
        far = arena(startsAt=NOW + 8 * 86_400_000)
        self.assertEqual(tournament_join.reason_to_skip(far, NOW), "longe demais")

    def test_iso_start_times_are_understood(self):
        # A API devolve o início em ms na lista da equipe e em ISO no detalhe do torneio.
        iso = arena(startsAt="2026-10-03T16:00:00Z")
        self.assertEqual(tournament_join.start_ms(iso), 1_791_043_200_000)
        self.assertIsNone(tournament_join.reason_to_skip(iso, 1_791_043_200_000 - 3_600_000))

    def test_an_arena_already_joined_is_skipped(self):
        self.assertEqual(tournament_join.reason_to_skip(arena(me={"rank": 0}), NOW), "já inscrito")


class TeamBattle(unittest.TestCase):
    def test_a_plain_arena_needs_no_team(self):
        self.assertEqual(tournament_join.team_for(arena(), ["darkonbot"]), (True, None))

    def test_a_team_battle_uses_one_of_our_teams_in_the_battle(self):
        battle = arena(teamBattle={"teams": {"other": "Other", "darkonbot": "DarkOnBot"}})
        self.assertEqual(tournament_join.team_for(battle, ["lichess-bots", "darkonbot"]), (True, "darkonbot"))

    def test_a_team_battle_without_our_teams_is_skipped(self):
        battle = arena(teamBattle={"teams": {"other": "Other"}})
        self.assertEqual(tournament_join.team_for(battle, ["darkonbot"]), (False, None))


if __name__ == "__main__":
    unittest.main()
