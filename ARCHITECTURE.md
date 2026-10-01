# Architecture

This document explains how the code is organized: where each thing lives, where a new thing goes, and the rules that keep the interpreter safe. Read it before adding or moving a file.

## Bird's-eye view

Slarz is a tree-walking interpreter, in a single crate with no dependencies. Running a script goes through a pipeline: each stage takes one form of the program and produces a more structured one.

```text
source text ─▶ lexer ─▶ tokens ─▶ parser ─▶ syntax tree ─▶ interpreter ─▶ output
                                                              │
                                              built-in functions (checked by permissions)
```

A static checker (types and permissions, before the script runs) will sit between the parser and the interpreter. Until then, types are checked while the script runs.

`src/lib.rs` holds the whole language. `src/main.rs` is a thin command line that calls it, so that other front ends (an MCP server, a playground) can reuse the same library.

## Code map

| Path | Holds | Never holds |
|---|---|---|
| `token.rs` | `Token`, `TokenKind`, `Span` (a position in the source), the keyword table, the foreign symbols (`&&`, `++`...) | messages |
| `lexer.rs` | `tokenize`: text to tokens, `LexError` | messages (they are in `diagnostic.rs`) |
| `ast.rs` | The syntax tree: `Program`, `Statement`, `Expression`, `Type`, `Permission`. Every node carries its `Span` | behavior |
| `parser.rs` | `parse`: tokens to syntax tree, one function per grammar rule, from the loosest operator to the tightest; `ParseError` | evaluation, messages |
| `value.rs` | `Value`: what a running script works with | operations on values |
| `interpreter.rs` | `run`, the `Interpreter` struct, `Flow` | anything else: it only wires the files below |
| `interpreter/statements.rs` | declarations, assignments, `if`, `while`, `for`, `return` | |
| `interpreter/expressions.rs` | `evaluate`: expressions to values, `check`, `otherwise` | |
| `interpreter/functions.rs` | declaring the script's functions, calling them or a built-in one | built-in functions themselves |
| `interpreter/environment.rs` | variables and scopes | |
| `interpreter/types.rs` | checking a value against a written type | |
| `interpreter/operators.rs` | what `+`, `==`, `<`, `not`... do | |
| `interpreter/error.rs` | `RuntimeError`, `RuntimeErrorKind` | messages |
| `builtins.rs` | **the table of every built-in function**, argument helpers | the functions themselves |
| `builtins/io.rs` | everything that reaches outside the script: screen, files, environment | pure computations |
| `builtins/{lists,numbers,text,json}.rs` | pure functions, one file per domain | any access to the outside world |
| `permissions.rs` | what a script may touch, resolved from its `permissions` block | |
| `diagnostic.rs` | **every message** of the errors that stop a script (lexer, parser, interpreter), the suggestions for foreign symbols, how an error is printed | logic that detects errors |
| `json.rs` | a standalone JSON parser and writer | any `use crate::...`: it depends on nothing else |

## Invariants

These rules carry the security promises. A change that breaks one is a bug, even if every test passes.

- **Only `builtins/io.rs` touches the outside world.** Reading or writing a file, the environment, the network: nowhere else. Each function there checks its permission before acting.
- **Pure built-in functions cannot reach the outside, by construction.** `Builtin::Pure` receives only its arguments; `Builtin::Io` receives a `Context` holding the output and the permissions. A pure function has nothing it could use to read a file.
- **A permission violation is never catchable.** It is `RuntimeErrorKind::PermissionDenied` or `ProtectedPath`; neither `check` nor `otherwise` can turn it into a value, and the command line exits with code 3.
- **Paths are compared once canonicalized, folder by folder**, never as text prefixes.
- **No `unsafe`, no `unwrap`, `expect` or `panic!` outside tests.** Enforced by `[lints]` in `Cargo.toml`; tests are allowed through `clippy.toml`.
- **No dependency** in the language crate without a written justification.

## Conventions

- **Files**: one responsibility each. A module that grows becomes `foo.rs` + `foo/`; never `mod.rs`.
- **Visibility**: `pub` only for what `main.rs` (or a future front end) needs; `pub(crate)` between top-level modules; `pub(super)` between the files of one module. A submodule can see the private items of its parent, so most helpers stay private.
- **Errors**: each stage has a `XxxError { kind, span }` and an `XxxErrorKind` enum. The kinds say *what* went wrong; their words live in `diagnostic.rs`.
- **Failures and absence in scripts**: a built-in that can fail returns `Value::Success` / `Value::Failure` (a `Result` in Slarz), with a message that says why, never the private value itself. A built-in whose answer can be "nothing" returns `Value::Present` / `Value::Absent` (an `Optional`).
- **Built-in names**: the Rust function has the Slarz name. Names state the decision they make: `round`/`floor`/`ceil` rather than `to_int`, `replace_all` rather than `replace`, `text_field`, `as_list`. The value worked on is always the first argument.
- **Comments**: English, rare, and only for a *why* that the code cannot say.

## How to add...

**A built-in function**
1. Write it in `builtins/<domain>.rs`, with the signature of its kind: `Pure` (`fn(Vec<Value>, Span)`), `Io` (`fn(&mut Context, Vec<Value>, Span)`), or `Quoted` (`fn(&Context, &[Expression], Span)`, only when the way an argument is written matters).
2. Add one line to the table in `builtins.rs`.
3. Test it in `interpreter/tests/<theme>.rs`, and with a golden script if a user would see it.

**An error that stops the script**
1. Add a variant to the stage's `XxxErrorKind`.
2. Write its message in `diagnostic.rs`: what went wrong, and how to fix it.
3. Test the exact message (a golden `error-*.slz` script is the clearest).

**Syntax**
1. `token.rs`: the token, and the keyword table if it is a word.
2. `ast.rs`: the node.
3. `parser.rs`: the rule, at the right precedence level.
4. `interpreter/statements.rs` or `interpreter/expressions.rs`: what it does. The compiler then lists every `match` to complete.
5. Tests in `parser.rs` (the tree it builds) and a golden script (what it does).

**A permission**
1. `ast.rs` (`PermissionKind`) and `parser.rs` (`permission`).
2. `permissions.rs`: how a declaration is resolved and checked.
3. The built-in that uses it, in `builtins/io.rs`, checking it before acting.

## Tests

| Where | What | When to add one |
|---|---|---|
| `#[cfg(test)] mod tests` in `lexer.rs`, `parser.rs`, `json.rs`, `permissions.rs` | a stage on its own | a rule of that stage |
| `interpreter/tests/<theme>.rs` | small scripts run in memory, checking what they print or the error that stops them | behavior of the language or of a built-in |
| `tests/scripts/name.slz` + `.out` / `.err` / `.code` | golden tests: the real `slarz` binary, compared character by character; scripts named `error-*` must fail | anything a user sees, every error message worth keeping |
| `tests/scripts.rs`, `examples_and_readme_run` | every `examples/*.slz` and every Slarz block of the README must run | automatic |

Data read by golden scripts lives in `tests/scripts/data/` (also listed in full by `folder-summary.slz`: adding a file there changes its output) or in a folder of its own, like `tests/scripts/api/`.

Before every commit: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
