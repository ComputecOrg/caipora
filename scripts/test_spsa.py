"""Testes do driver de SPSA (`spsa.py`): só as partes puras, sem engine nem fastchess.

    python -m unittest discover -s scripts -p "test_spsa.py"
"""

import argparse
import os
import random
import subprocess
import sys
import unittest

import spsa

TUNE_OUTPUT = """rfp_margin, int, 80, 30, 200, 8, 0.002
lmp_base, int, 3, 0, 10, 1, 0.002
"""

FASTCHESS_OUTPUT = """Results of plus vs minus (8+0.08, 1t, 16MB, 8moves_v3.epd):
Elo: 34.86 +/- 120.21, nElo: 50.00 +/- 172.40
Games: 8, Wins: 3, Losses: 2, Draws: 3, Points: 4.5 (56.25 %)
Results of plus vs minus (8+0.08, 1t, 16MB, 8moves_v3.epd):
Elo: 0.00 +/- 0.00, nElo: 0.00 +/- 0.00
Games: 16, Wins: 7, Losses: 4, Draws: 5, Points: 9.5 (59.38 %)
"""


class ParseTest(unittest.TestCase):
    def test_tune_output_becomes_parameters(self):
        params = spsa.parse_tune_output("id name Caipora\n" + TUNE_OUTPUT)
        self.assertEqual([p.name for p in params], ["rfp_margin", "lmp_base"])
        rfp = params[0]
        self.assertEqual(
            (rfp.value, rfp.min, rfp.max, rfp.c_end, rfp.r_end), (80, 30, 200, 8, 0.002)
        )

    def test_fastchess_result_is_the_last_summary(self):
        self.assertEqual(spsa.parse_result(FASTCHESS_OUTPUT), (7, 4, 5))

    def test_missing_result_is_an_error(self):
        with self.assertRaises(ValueError):
            spsa.parse_result("nada aqui")


class StepTest(unittest.TestCase):
    def setUp(self):
        self.params = spsa.parse_tune_output(TUNE_OUTPUT)

    def test_perturbation_is_symmetric_and_inside_the_range(self):
        rng = random.Random(1)
        signs, plus, minus = spsa.perturb(self.params, iteration=1, total=100, rng=rng)
        for p, s, hi, lo in zip(self.params, signs, plus, minus):
            self.assertIn(s, (-1, 1))
            if s > 0:
                self.assertTrue(p.min <= lo <= hi <= p.max)
            else:
                self.assertTrue(p.min <= hi <= lo <= p.max)
            self.assertNotEqual(hi, lo)

    def test_final_perturbation_is_c_end(self):
        c = spsa.schedule(self.params[0], iteration=100, total=100)[0]
        self.assertAlmostEqual(c, 8.0)

    def test_update_moves_toward_the_winning_side(self):
        signs = [1, -1]
        before = [p.value for p in self.params]
        spsa.update(self.params, signs, wins=10, losses=2, iteration=1, total=100)
        self.assertGreater(self.params[0].value, before[0])
        self.assertLess(self.params[1].value, before[1])

    def test_update_stays_inside_the_range(self):
        for _ in range(50):
            spsa.update(self.params, [1, 1], wins=100, losses=0, iteration=1, total=100)
        self.assertEqual(self.params[0].value, 200)
        self.assertEqual(self.params[1].value, 10)


class CommandTest(unittest.TestCase):
    def test_each_side_sets_the_tunables_as_uci_options(self):
        params = spsa.parse_tune_output(TUNE_OUTPUT)
        args = argparse.Namespace(
            engine="caipora.exe", fastchess="fastchess.exe", book="book.epd",
            tc="8+0.08", hash=16, pairs=4, concurrency=5,
        )
        command = spsa.fastchess_command(args, params, [88, 4], [72, 2], seed=7)
        plus = command.index("name=plus")
        minus = command.index("name=minus")
        self.assertEqual(command[0], "fastchess.exe")
        self.assertEqual(command[plus - 1], "cmd=caipora.exe")
        self.assertEqual(command[plus + 1 : plus + 3], ["option.rfp_margin=88", "option.lmp_base=4"])
        self.assertEqual(
            command[minus + 1 : minus + 3], ["option.rfp_margin=72", "option.lmp_base=2"]
        )
        # Pares de partidas com a mesma abertura e as cores trocadas.
        self.assertIn("-repeat", command)
        self.assertEqual(command[command.index("-rounds") + 1], "4")
        self.assertEqual(command[command.index("-srand") + 1], "7")


class CliTest(unittest.TestCase):
    def test_help_runs_and_documents_the_session(self):
        script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "spsa.py")
        run = subprocess.run(
            [sys.executable, script, "--help"], capture_output=True, text=True,
            encoding="utf-8", check=False,
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        for expected in ("--features tune", "--engine", "--fastchess", "option.<nome>"):
            self.assertIn(expected, run.stdout)


if __name__ == "__main__":
    unittest.main()
