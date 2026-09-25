# Slarz

**A scripting language designed for AI agents: easy for an AI to write, easy for a human to review, and unable to do anything it was not allowed to.**

> 🚧 Early development. Nothing runs yet. This project is built in public, one step at a time.

## Why

AI agents increasingly write and run scripts on our machines. A generated script can delete files or send data anywhere, and nobody really reads it before it runs.

Slarz takes a different approach: every script declares its permissions up front, and the interpreter blocks everything else. To trust a script, a human reviews a few lines instead of the whole program.

## Design principles

- **Explicit permissions.** Access to files, network and environment is denied by default and declared in the script.
- **One way to write each thing.** Strict typing, no implicit behavior, no ambiguity.
- **Errors written for machines and humans.** Every error says what went wrong, where, and how to fix it, so an AI can correct itself.
- **Readable.** Our hypothesis is that AIs fail because of ambiguity, not verbosity. The benchmark will test it.

## Roadmap

1. **Language v0**: a tree-walking interpreter in Rust, based on [*Crafting Interpreters*](https://craftinginterpreters.com/)
2. **Benchmark**: which AI learns a never-seen-before language fastest, from its documentation alone?
3. **MCP server**: let any agent write, check and run Slarz code in a sandbox
4. **Fine-tuning**: train a small open model on interpreter-verified examples

## Building

Requires [Rust](https://www.rust-lang.org/tools/install).

```sh
cargo run
```
