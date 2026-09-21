---
name: verified-module-split
description: Decide whether a large file should be split, then split it without changing behaviour — judge by whether the boundary already exists elsewhere in the tree, move code mechanically, and prove zero change with a normalized line diff. Use when asked to tidy code, to keep responsibilities separate, or when the question is "should this file be split".
---

# Splitting a module without changing it

## The judgement comes first

Line count is not the criterion. The criterion is: **does this boundary already exist somewhere
else in the tree?** If the rest of the tree has already drawn it, splitting makes this layer agree
with its own boundaries — an argument that can be quoted and refuted. If the boundary exists only
in your head, do not split.

Gather evidence before touching anything:

- **The module doc contradicts the file.** A `//!` describing one half of a file that has two is
  direct evidence of a merge. (`update.rs` documented the registry check while it also held the
  shell's own release check.)
- **The two halves share no code and never call each other.** Grep every private helper for its
  callers. If they do call each other, it is not a clean cut.
- **Someone else already separates them.** Two core modules, two groups of commands, two payloads,
  two events, a `self_` field prefix — record the `file:line` of each. Here: `updates` vs
  `self_update` in `crates/dsh-core`, `check_updates`/`check_self_update` in `src-tauri/src/commands.rs`,
  the `self_` fields in `src-tauri/src/state.rs`, and the two payload events.
- **No consumer uses one half alone.** If nobody would ever import just a part, the two files will
  always be opened together and the split only adds depth.

When the answer is "do not split", give the reason that can be refuted:

- One subject with a straight-line data flow (`fetch → parse → pick → download → verify`).
- A **view controller**, whose cohesion is a shared mutable snapshot rather than a shared subject.
  Splitting means threading that state through arguments (a rewrite) or sharing mutable bindings
  across modules (worse). `web/main.js` is this case.
- A **composition root** (`src-tauri/src/lib.rs`): wiring, registration, assembly. Moving a piece
  of it moves wiring away from the only place that wires.
- Only pure leaf helpers are separable — the gain is smaller than the import it costs.

## Move the code mechanically

Write a throwaway script (kept outside the repository) rather than retyping code by hand.

- Cut on **top-level item boundaries** (`fn` / `struct` / `enum` / `const` / `impl`), never on line
  numbers.
- Move comments and code verbatim; let `cargo fmt` do any reformatting afterwards.
- Tests follow the code they test, each new module taking its own `#[cfg(test)] mod tests`.
- Dry-run first and print the assignment — what goes where, how many lines each file gets — then
  write. Two traps in a `mod tests` block: an attribute line left behind at the end of the previous
  test, and the block's closing brace counted as part of the last test.
- Keep the script: it is the record of what moved, and it can be re-run.

Dependencies must run one way. Say which way in the new `lib.rs` / `mod.rs` — which module depends
on which, and for each helper placed at the top rather than the bottom, why (usually to avoid an
A↔B cycle). See `crates/dsh-core/src/lib.rs` and `src-tauri/src/update/mod.rs` for the shape.

## Prove the change is zero

```sh
git show HEAD:<old> > /tmp/old.rs              # keep a copy before deleting the file
normalize() { grep -vE '^\s*(//|//!|use |$)' "$1" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'; }
normalize /tmp/old.rs | sort > /tmp/a.txt
{ normalize new1.rs; normalize new2.rs; } | sort > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt
```

The only lines that may differ are the continuation lines of a multi-line `use` in the old file —
the `use |` filter cannot see them. Anything else means behaviour was touched: revert and redo,
do not "fix" it in place.

## Compile, verify, commit

The expected errors are all imports and visibility:

- A constant shared across modules becomes `pub(crate)`; inside one module tree, `pub(super)`
  rather than something wider.
- `crates/dsh-core/examples/*.rs` name modules by path too. `cargo check --all-targets` is what
  catches those, and they are easy to forget because the library builds without them.
- Re-point references from a mapping script rather than by hand.

Then the repository's own list — `AGENTS.md` → Verify. Test counts and assertions must not move; if
a number changes, behaviour changed.

Commit:

- One commit per subject, with the verification run recorded in the message.
- Long messages go in a file, committed with `git commit -F <file>`: a multi-line `-m` through a
  shell eats backticks, and message text containing shell metacharacters can be intercepted.
- If a commit dies with no output at all, suspect a slow pre-commit hook rather than a policy
  block. `git commit --allow-empty` succeeding while the real commit dies tells them apart; retry
  with a longer timeout.

## Anti-patterns

- Changing logic, comments or names while moving — the diff can then prove nothing.
- Reporting "the file got smaller" as the result. Report the boundary and the direction of the
  dependencies.
- Inventing an 80-line module because some number was exceeded.
