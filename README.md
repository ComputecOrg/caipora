# Caipora

Caipora is a UCI chess engine written in Rust, in early development.

It is not playable yet. The foundation (board representation, FEN in standard, X-FEN and
Shredder-FEN notation, Chess960-ready castling rights) is being built test-first; move
generation validated by perft comes next.

## AI usage

Caipora is written by [Claude Code](https://claude.com/claude-code) (Anthropic) under the
direction, review and testing of its author. Every commit is co-authored by Claude. See
[AI_USAGE.md](AI_USAGE.md) for the full policy, including training data and originality rules.

## Building

Requires Rust (the exact toolchain is pinned in `rust-toolchain.toml`).

```
cargo build --release
cargo test
```

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
