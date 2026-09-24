---
name: ci-regression-triage
description: Decide whether a red CI leg is your regression, and fix the leg if it is not. Use when a job fails that looked green before, when the failing step prints nothing, or when the same job and the same spec disagree across days.
agent_created: true
---

# Triaging a red leg

A failing leg is a claim, not a fact. Before touching the code it points at, separate
three things: **what failed** (the step must say so), **whether it is new** (the same
job on an older commit says so), and **whether it is yours at all** (the failing
process is often not the one you changed).

## 1. Make the step say what happened

A step that redirects its command's output into a file and only prints that file from
checks *after* it will fail with an empty log when the command fails first. That is
the worst possible state: the job is red, the log blames nothing, and the reason has
to be reconstructed by hand.

```bash
fail() {
  echo "::error::$1"
  cat "$out" || true
  for log in "${DSH_HOME:-$HOME/.dsh}"/launcher/logs/server-*.err.log; do
    [ -f "$log" ] || continue
    echo "──── ${log##*/} ────"; tail -n 40 "$log" || true
  done
  exit 1
}
cargo run … > "$out" 2>&1 || fail "the launch exited non-zero"
```

Print the *child's* stderr too, not just your own wrapper's. In this repository the
spawned dsh writes its refusal to `server-<port>.err.log` and nowhere else, so a
"server never came up" reads as a handshake bug until that file is in the log.

**A step with no output is a bug in the step.** Fix it before diagnosing further —
that change is worth keeping regardless of what it reveals.

## 2. Look for the job on other commits before blaming the diff

Cheap and decisive, and usually already paid for:

- `gh run list --workflow <wf> --branch <default> --limit 10` — **scheduled runs on the
  default branch ran the code as it was before your change**. If the same leg is red
  there, the diff is not the cause. (Here: `compat` runs on `23 3 * * *`, and the
  08:41Z scheduled run on `main` was already red on the commit before the fix.)
- `gh run view <run> --json jobs --jq '.jobs[] | "\(.conclusion)  \(.name)"'` — compare
  *whole matrices* rather than one leg: `@latest` green while `@0.1.5-rc.1` is red says
  the failure is version-specific, not code-wide.
- `gh run list --branch <branch>` back through several pushes: the first red commit is
  where the environment or the locked dependency drifted, which is not necessarily the
  commit that introduced the code change.

## 3. Reproduce the environment, not just the call

`gh run view --job <id> --log` gives the child's error verbatim; take that string and
reproduce it locally with the same install command the job uses:

```bash
npm i -g --prefix /tmp/pfx "@scope/pkg@x.y.z"      # same spec as the job
DSH_BIN=/tmp/pfx/node_modules/@scope/pkg/lib/bin.js \
DSH_HOME=/tmp/home cargo run --example launch      # same entry point as the job
```

An explicit `DSH_BIN` beats `PATH`, so the run is unaffected by whatever else is
installed on the machine.

## 4. A bare specifier is a moving tree

`pkg@1.2.3-rc.1` installs rc.1 of the *top-level* package and the newest of everything
it ranges over. Prerelease ranges (`^1.2.3-rc.1`) stay inside that `x.y.z`, but they do
move: publishing rc.2 or rc.3 changes what the rc.1 install resolves to, and that can
break the tree without anyone touching rc.1. Symptoms: the same job and the same spec
disagree across days; the failure is in the dependency's own layout (`X could not be
resolved`), and the version the job reports is still the pinned one.

Pin the resolution, not just the spec:

```bash
npm i -g "@scope/pkg@1.2.3-rc.1" --before=2026-09-11T00:00:00Z
```

`--before` installs the tree as it stood on that date — which is what the job's name
promises when the job is a compatibility *floor*. Say why in the step, name the date
and the release that changed the resolution, and verify both ways locally (with the
pin: boots; without: does not).

## 5. Report the boundary honestly

When the failure is upstream's, say so with the artifact that proves it (the child's
error, the two publish dates, the local A/B) and keep the leg meaningful instead of
pinning it to a version that happens to work. A red leg that gets deleted loses the
coverage that would have caught the next one.
