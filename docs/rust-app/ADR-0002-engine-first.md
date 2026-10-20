# ADR 0002: engine first, shell second

- **Status:** accepted
- **Date:** 2026-10-20
- **Deciders:** Sato

## Context

`src/` is 796 files and about 298,000 lines of C++: the project and track
model, audio I/O, file formats, effects, commands and the wxWidgets shell. A
big-bang GUI rewrite would put the shipping editor at risk and would leave no
way to test new audio code against the old interface.

## Decision

Move the engine to Rust first and keep the C++ frontend working:

1. Port audio I/O, formats, the project model, blockfiles, undo and the
   effects host into Rust crates that expose the existing C ABI where the C++
   application calls them.
2. Build the iced shell against those crates directly, with no C ABI in the
   Rust path.
3. Reach MVP parity, then full parity, and delete `src/` and the C++ build.

The C++ application remains the shipping application until the Rust shell
passes the parity matrix. `lib-src/` stays vendored C/C++ and is not part of
the rewrite.

## Consequences

- The old and new frontends share one engine, so audio fixes land once.
- The engine needs a stable, documented boundary earlier than a GUI-first
  rewrite would.
- `.aup` compatibility is a first-class requirement, not an afterthought.
- The repository carries two frontends during the transition; the parity
  matrix is the exit condition.

## Alternatives

- **GUI first** — fastest visible progress, but the new shell would sit on the
  old engine and the riskiest code (audio, formats, project) would move last.
- **In-place rewrite on `main`** — simpler repository, but `main` would stop
  being a usable editor for the duration.
