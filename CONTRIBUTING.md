# Contributing

Thanks for helping with speedal. This is a Rust workspace: libraries do the
inference, `speedal-cli` is a thin frontend. A later UI should call
`speedal-session`, not the CLI.

## Development

Rust 1.85+ (edition 2024).

```bash
cargo test --workspace
cargo run --release -- --help
```

First `both` / `asr` / `diar` / `enroll` run downloads ONNX weights into the
Hugging Face cache. Do not commit `.onnx` files, `target/`, or speaker
galleries (`speakers.json`).

On macOS, grant system-audio capture if you use `--listen`.

## Pull requests

1. Open an issue first for large changes.
2. Keep diffs focused. Match the existing crate split
   (`speedal-audio`, `speedal-asr`, `speedal-diar`, `speedal-spk`,
   `speedal-session`, `speedal-cli`).
3. Add or update tests when you change alignment, gallery, or CLI parsing.
4. Run `cargo test --workspace` before you open the PR.
5. Use [Conventional Commits](https://www.conventionalcommits.org/) if you can
   (`fix:`, `feat:`, `docs:`).

By contributing, you agree that your work is dual-licensed MIT OR Apache-2.0,
same as the rest of the repo. See [LICENSE](LICENSE).

## Code of conduct

Participation is covered by the [Contributor Covenant](CODE_CONDUCT.md).
