[English](CONTRIBUTING.md) | [日本語](CONTRIBUTING.ja.md)

# 開発者向けガイド

配布版を使わず自分でビルドしたい場合や、開発に参加する場合は以下の通りです。[Rust](https://www.rust-lang.org/) と [Node.js](https://nodejs.org/) が必要です。

## ソースからビルドする

```sh
# 開発時の起動
cd src-tauri
cargo run

# 配布用アプリのビルド
npx @tauri-apps/cli build
# → macOS: src-tauri/target/release/bundle/dmg/ 以下
# → Windows: src-tauri/target/release/bundle/msi/ 以下
```

## テスト

テストの実行（実 API への送信は行わず、すべてローカルで完結します）:

```sh
cd src-tauri
cargo test
```

## 設計方針

設計方針は [docs/design_doc.md](docs/design_doc.md) を参照してください。

## Issue・プルリクエスト

このアプリは OSS として公開しています。**気になった点は、どんなことでも気軽に [Issue](https://github.com/kenshin-morioka/instagram-crm/issues) を立ててください。**

- 「インストールでつまずいた」「この説明が分かりにくい」といった **初歩的な内容でも大歓迎** です
- バグ報告・機能要望・改善提案、いずれも歓迎します
- 「こういう使い方をしたい」という相談も気軽にどうぞ

プルリクエストも歓迎します 🙌
