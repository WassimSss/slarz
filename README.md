# Slarz

**A scripting language designed for AI agents: easy for an AI to write, easy for a human to review, and unable to do anything it was not allowed to.**

> 🚧 Early development. Nothing runs yet. This project is built in public, one step at a time.

## Why

AI agents increasingly write and run scripts on our machines. A generated script can read your secrets, delete files or send data anywhere. To be sure it won't, you would have to read every line, and nobody does.

This is not hypothetical. In 2025, an AI agent deleted a production database during a code freeze, a malicious npm package turned local AI coding assistants into secret-stealing tools, and hidden instructions in a support ticket made an agent leak private database tokens.

## How it works

Slarz is a language plus a harness around it.

- **The script asks.** Every script declares up front what it needs: which folders it reads or writes, which websites it calls. A human can review that in seconds instead of reading the whole program.
- **The human grants.** Authorization lives outside the script, in a policy only the human controls. A script runs only if what it asks for is within what was granted. The policy is the ceiling of what can ever go wrong, even if the AI was tricked into writing a malicious script.
- **Checked before it runs.** Scripts are statically checked, so a script that breaks the rules is rejected before it touches anything, not halfway through.
- **No AI at run time.** A script is deterministic code. Once written and approved, it can run every day without an AI in the loop, so instructions hidden in the data it processes cannot change what it does.

## Design principles

- **Deny by default.** No filesystem, network or environment access unless declared and granted.
- **Friction proportional to risk.** Safe actions pass silently; new or dangerous ones trigger a clear question. Unattended runs never ask: anything not pre-approved is denied.
- **No escape hatches.** No "skip all permissions" flag, no foreign function interface, no unsafe blocks.
- **One way to write each thing.** Strict typing, no implicit behavior, no ambiguity.
- **Errors written for machines and humans.** Every error says what went wrong, where, and how to fix it, so an AI can correct itself.
- **Readable.** Our hypothesis is that AIs fail because of ambiguity, not verbosity. The benchmark will test it.

## What Slarz does not do

Slarz does not make AI models trustworthy. It cannot stop a model from being manipulated, and it does not cover actions an agent takes without writing a script. What it does is bound what a script can do, and make that visible.

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

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
