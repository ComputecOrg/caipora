# Caipora

Caipora is a UCI chess engine written in Rust.

## Status

First playable version (0.1.0):

- Legal move generation for standard chess and Chess960/DFRC, validated by the full standard
  (128 positions) and Chess960 (960 positions) perft suites.
- Alpha-beta search: iterative deepening, aspiration windows, PVS, check extension, quiescence
  search, transposition table, TT move and MVV-LVA ordering, draw detection (repetition,
  fifty-move rule, insufficient material), soft/hard time management.
- Evaluation: NNUE, (768×8 king buckets → 1024)×2 → 8 output buckets with SCReLU, embedded in the
  binary. Networks up to g3
  were trained only on Caipora's self-play; the current network (g6) was trained on Leela Chess
  Zero training data, available under the Open Database License (ODbL). See [AI_USAGE.md](AI_USAGE.md).
- Lazy SMP (shared lock-free transposition table) and pondering.
- UCI options: `Hash`, `Threads` (up to 256), `Move Overhead`, `UCI_Chess960`, `Clear Hash`,
  `EvalFile`, `Ponder`.
  Extra commands: `bench [depth]`, `go perft N`, `d`, `eval`.

## AI usage

Caipora is written by [Claude Code](https://claude.com/claude-code) (Anthropic) under the
direction, review and testing of its author. Every commit is co-authored by Claude. See
[AI_USAGE.md](AI_USAGE.md) for the full policy, including training data and originality rules.

## Building

Requires Rust (the exact toolchain is pinned in `rust-toolchain.toml`).

```
cargo build --release
./target/release/caipora bench     # prints "Bench: <nodes> nodes <nps> nps"
```

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
