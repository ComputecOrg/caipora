"""Inscreve o bot nos torneios (arenas) abertos a bots das equipes dele.

Roda de hora em hora no servidor (timer systemd `caipora-tournaments`). Só entra em arena de
xadrez padrão, rated, com bots liberados, ritmo de blitz a clássica curta (duração estimada de 3 a
50 minutos, base + 40 × incremento: sem bullet), que
ainda não começou e começa em até 7 dias. Em team battle, joga por uma das equipes do bot que
estiver na disputa. O lichess-bot joga as partidas sozinho: o torneio chega como mais um
`gameStart`.

    LICHESS_BOT_TOKEN=... python3 tournament_join.py [--dry-run]

O token precisa de `tournament:write` (inscrever) e `team:read`/`team:write` (equipes do bot).
"""

import argparse
import datetime
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://lichess.org"
# Duração estimada (base + 40 lances de incremento, como o Lichess classifica): de blitz (3 min)
# até 50 minutos; abaixo é bullet, acima prende o bot por horas.
MIN_ESTIMATED_SECONDS = 180
MAX_ESTIMATED_SECONDS = 3000
HORIZON_MS = 7 * 86_400_000
CREATED = 10


def start_ms(tournament: dict) -> int:
    """Início em ms: a lista da equipe dá número; o detalhe do torneio, texto ISO."""
    start = tournament.get("startsAt")
    if isinstance(start, (int, float)):
        return int(start)
    when = datetime.datetime.fromisoformat(str(start).replace("Z", "+00:00"))
    return int(when.timestamp() * 1000)


def variant_key(tournament: dict) -> str:
    variant = tournament.get("variant", "standard")
    return variant.get("key", "") if isinstance(variant, dict) else str(variant)


def reason_to_skip(tournament: dict, now_ms: int) -> str | None:
    """`None` se o bot deve entrar; senão, o motivo (vai para o log)."""
    if not tournament.get("botsAllowed"):
        return "sem bots"
    if variant_key(tournament) != "standard":
        return "variante"
    if not tournament.get("rated"):
        return "casual"
    clock = tournament.get("clock", {})
    estimated = clock.get("limit", 0) + 40 * clock.get("increment", 0)
    if not MIN_ESTIMATED_SECONDS <= estimated <= MAX_ESTIMATED_SECONDS:
        return "ritmo"
    if tournament.get("isFinished") or tournament.get("status", CREATED) != CREATED:
        return "já começou"
    if start_ms(tournament) - now_ms > HORIZON_MS:
        return "longe demais"
    if tournament.get("me") is not None:
        return "já inscrito"
    return None


def team_for(tournament: dict, our_teams: list[str]) -> tuple[bool, str | None]:
    """(pode entrar, equipe): arena comum não pede equipe; team battle, uma das nossas na disputa."""
    battle = tournament.get("teamBattle")
    if not battle:
        return True, None
    teams = battle.get("teams", {})
    for team in our_teams:
        if team in teams:
            return True, team
    return False, None


def soonest_first(tournaments: list[dict]) -> list[dict]:
    """O Lichess limita as inscrições por período: as vagas vão primeiro para os mais próximos."""
    return sorted(tournaments, key=start_ms)


def is_join_limit(result: dict) -> bool:
    return "too many tournaments" in str(result.get("error", ""))


class Lichess:
    def __init__(self, token: str):
        self.token = token

    def _request(self, path: str, data: dict | None = None, ndjson: bool = False):
        body = urllib.parse.urlencode(data).encode() if data is not None else None
        headers = {
            "Authorization": f"Bearer {self.token}",
            "Accept": "application/x-ndjson" if ndjson else "application/json",
        }
        for _ in range(3):
            try:
                request = urllib.request.Request(API + path, data=body, headers=headers)
                with urllib.request.urlopen(request, timeout=30) as response:
                    text = response.read().decode()
                time.sleep(1)  # educado com a API
                if ndjson:
                    return [json.loads(line) for line in text.splitlines() if line.strip()]
                return json.loads(text) if text else {}
            except urllib.error.HTTPError as error:
                if error.code == 429:
                    time.sleep(65)
                    continue
                return {"error": f"HTTP {error.code}: {error.read().decode()[:200]}"}
        return {"error": "limite da API"}

    def my_teams(self, user: str) -> list[str]:
        teams = self._request(f"/api/team/of/{user}")
        return [t["id"] for t in teams] if isinstance(teams, list) else []

    def team_arenas(self, team: str) -> list[dict]:
        arenas = self._request(f"/api/team/{team}/arena?max=30", ndjson=True)
        return arenas if isinstance(arenas, list) else []

    def tournament(self, tournament_id: str) -> dict:
        return self._request(f"/api/tournament/{tournament_id}")

    def join(self, tournament_id: str, team: str | None) -> dict:
        return self._request(f"/api/tournament/{tournament_id}/join", {"team": team} if team else {})

    def user(self) -> str:
        return self._request("/api/account").get("id", "")


def run(lichess: Lichess, dry_run: bool) -> int:
    now = int(time.time() * 1000)
    me = lichess.user()
    teams = lichess.my_teams(me)
    print(f"{datetime.datetime.now():%Y-%m-%d %H:%M} {me}: equipes {teams}")
    joined = 0
    candidates = {}
    for team in teams:
        for listed in lichess.team_arenas(team):
            if listed.get("status") == CREATED and start_ms(listed) - now <= HORIZON_MS:
                candidates[listed["id"]] = listed
    for listed in soonest_first(list(candidates.values())):
        tid = listed["id"]
        full = lichess.tournament(tid)
        name = full.get("fullName", tid)
        reason = reason_to_skip(full, now)
        if reason is None:
            allowed, battle_team = team_for(full, teams)
            if not allowed:
                reason = "team battle sem equipe nossa"
        if reason:
            print(f"  pula {tid} ({name}): {reason}")
            continue
        if dry_run:
            print(f"  entraria em {tid} ({name})")
            continue
        result = lichess.join(tid, battle_team)
        if result.get("ok"):
            joined += 1
            print(f"  entrou em {tid} ({name})" + (f" pela equipe {battle_team}" if battle_team else ""))
        else:
            print(f"  falhou {tid} ({name}): {result.get('error', result)}")
            if is_join_limit(result):
                print("  limite de inscrições do Lichess; o resto fica para a próxima rodada")
                break
    return joined


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dry-run", action="store_true", help="só mostra o que faria")
    args = parser.parse_args(argv)
    token = os.environ.get("LICHESS_BOT_TOKEN")
    if not token:
        print("falta LICHESS_BOT_TOKEN", file=sys.stderr)
        return 2
    run(Lichess(token), args.dry_run)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
