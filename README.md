<p align="center">
  <img src="assets/logo.svg" alt="Slarz logo: a castle gate" width="120">
</p>

# Slarz

**A scripting language designed for AI agents: easy for an AI to write, easy for a human to review, and unable to do anything it was not allowed to.**

> 🚧 Early development: the interpreter runs its first scripts, but permissions are not enforced yet. This project is built in public, one step at a time.

## Why

AI agents increasingly write and run scripts on our machines. A generated script can read your secrets, delete files or send data anywhere. To be sure it won't, you would have to read every line, and nobody does.

This is not hypothetical. In 2025, an AI agent deleted a production database during a code freeze, a malicious npm package turned local AI coding assistants into secret-stealing tools, and hidden instructions in a support ticket made an agent leak private database tokens.

## What Slarz is for

Slarz is for the everyday tasks an AI agent does on your behalf, on your files and accounts: reading, transforming and sending data.

- Sorting and renaming files
- Turning data into other data: CSV, JSON, reports
- Calling APIs with a secret key
- Recurring automations: every morning, fetch this and produce that
- Connecting two services together

The same script behaves the same way on Windows, macOS and Linux: the AI writing it does not need to know which system it runs on.

Slarz is **not** a general-purpose language. It is not meant for building software (that requires running compilers and other programs, which Slarz forbids on purpose), user interfaces or games.

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

## Language decisions

The core semantics of v0 are settled. Each rule removes a common source of bugs or ambiguity.

- **Statically typed, no escape hatch.** Type errors are caught before the script runs. There is no `any` type; data of unknown shape, like JSON, goes through an explicit type that must be inspected.
- **Types are always written.** Variables, parameters and return values are annotated. The AI does the writing; the reader never has to guess.
- **No null.** A variable always has a value from the moment it is declared. When an answer can be "none" (an unset environment variable, a missing JSON field), the type says so and the script must handle it.
- **No silent failures.** Operations that can fail say so in their type, and the failure must be handled. A permission violation stops the script immediately and cannot be caught, so a script cannot probe what it is allowed to do.
- **Immutable by default, copies on assignment.** A value never changes behind your back.
- **One equality.** Values are compared by content, only between values of the same type, with no hidden conversions.
- **No surprising arithmetic.** Integer overflow and division by zero stop the script. Dividing two integers is not allowed with `/`, because languages disagree on whether `7 / 2` is `3` or `3.5`.
- **Left-to-right evaluation, guaranteed.** Side effects always happen in the order they are written.

## Security layers

Permissions are enforced in depth:

1. **The interpreter**: a script can only reach the outside world through built-in functions, and each one checks its permission before acting. There is no way to start an external process.
2. **The operating system**: the interpreter will sandbox itself at startup (starting with Landlock on Linux), so that even a bug in the interpreter cannot escape the granted permissions. The MCP server will also run inside a container.

v0 ships with layer 1 only.

## What Slarz does not do

Slarz does not make AI models trustworthy. It cannot stop a model from being manipulated, and it does not cover actions an agent takes without writing a script. What it does is bound what a script can do, and make that visible.

## Roadmap

1. **Language v0**: a tree-walking interpreter in Rust, based on [*Crafting Interpreters*](https://craftinginterpreters.com/)
2. **Benchmark**: which AI learns a never-seen-before language fastest, from its documentation alone?
3. **MCP server**: let any agent write, check and run Slarz code in a sandbox
4. **Fine-tuning**: train a small open model on interpreter-verified examples

## Status

| Stage | State |
|---|---|
| Lexer | ✅ done |
| Parser | ✅ done |
| Interpreter | ✅ variables, functions, `if`, `while`, `print` |
| Type checker | 🔜 types are checked while the script runs for now |
| Permissions | 🔜 the header is parsed but not enforced yet |
| Lists, `for`, files, network | 🔜 |

## Installation

There are no prebuilt binaries yet. Install from source with [Rust](https://www.rust-lang.org/tools/install):

```sh
cargo install --git https://github.com/WassimSss/slarz
```

## Usage

```sh
slarz script.slz
```

A script starts with its permissions, then does its work:

```
permissions { }

rate: Float = 0.2;
var total: Float = 0.0;

function with_tax(amount: Float) -> Float {
    return amount + amount * rate;
}

total = total + with_tax(100.0);
print(total);
```

More in [`examples/`](examples/).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
