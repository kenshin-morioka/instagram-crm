[English](CONTRIBUTING.md) | [日本語](CONTRIBUTING.ja.md)

# Developer guide

Read this if you want to build the app yourself instead of using the released binaries, or if you want to contribute. You need [Rust](https://www.rust-lang.org/) and [Node.js](https://nodejs.org/).

> **Note:** the linked documents under `docs/` are currently available in Japanese only.

## Building from source

```sh
# run in development mode
cd src-tauri
cargo run

# build the distributable app
npx @tauri-apps/cli build
# → macOS: under src-tauri/target/release/bundle/dmg/
# → Windows: under src-tauri/target/release/bundle/msi/
```

## Tests

Running the tests (nothing is sent to the real API; everything runs locally):

```sh
cd src-tauri
cargo test
```

## Design

See [docs/design_doc.md](docs/design_doc.md) for the design decisions behind the app.

## Issues and pull requests

This app is published as open source. **If anything catches your attention, please feel free to open an [Issue](https://github.com/kenshin-morioka/instagram-crm/issues) about it — anything at all.**

- Basic things like "I got stuck during installation" or "this explanation is hard to follow" are **very welcome**
- Bug reports, feature requests, and improvement suggestions are all welcome
- Questions like "I'd like to use it this way — is that possible?" are welcome too

Pull requests are welcome as well 🙌
