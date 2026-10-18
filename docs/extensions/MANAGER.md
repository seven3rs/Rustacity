# The extension manager

Two programs that do one job: say which extensions are installed for Rustacity,
turn one on or off, and ask the extension host whether one still loads what it
claims to.

| program | what it is |
| --- | --- |
| `rustacity-ext-manager-gui` | the window |
| `rustacity-ext-manager` | the same work without a window: it prints the listing |

Neither is part of the editor. Nothing in `src/` reads the extensions directory
yet — the C++ class that would run an extension exists and is linked in, and no
discovery module constructs one (§4 of [INTEGRATION_NOTES.md](INTEGRATION_NOTES.md))
— so installing an extension through the manager does not make it appear in the
editor's menus. What is finished here is the directory half of that job, with
tests, so that the editor half has something settled to read.

## Where extensions live

`<user data directory>/Extensions`, beside the `Plug-Ins` directory the
application already keeps there:

| system | directory |
| --- | --- |
| Windows | `%APPDATA%\rustacity\Extensions` |
| Linux | `~/.rustacity-data/Extensions` |
| macOS | `~/Library/Application Support/rustacity/Extensions` |

`RUSTACITY_EXTENSIONS_DIR` overrides that. The rules for the parent directory are
copied from `src/FileNames.cpp` rather than invented a second time, so the two
programs agree about the location without sharing a setting.

An extension is a directory holding `extension.toml` and `extension.wasm`,
which is what [WRITING_AN_EXTENSION.md](WRITING_AN_EXTENSION.md) §6 builds
towards. So **installing one is putting that directory in the root** — by hand,
or by copying it out of `rust/extensions/dist/` after
`scripts/build-extensions.sh`.

**Turning one off moves it into a `disabled` subdirectory of the root**, and
turning it on moves it back:

```
Extensions/
  gain-invert/          installed
  disabled/
    template/           turned off
  notes.txt             not an extension, and the manager says so
```

The state is the location. There is no list of turned-off extensions in a
setting, a database or a dotfile, nothing has to be kept in step, and the whole
of it can be read and changed in a file manager. Both directories are children
of the same root, so each move is a rename: it either happens or does not, and
the opposite call undoes it exactly.

## The window

`rustacity-ext-manager-gui` opens on the root and shows:

- the directory, and how many extensions are installed and how many are turned
  off;
- the two lists, with the name the manifest gives each extension and the name of
  its directory beside it — the directory is what every action addresses, and it
  is often the only thing telling two similarly named extensions apart;
- a filter box, and a Refresh button, because the directory can also be changed
  in a file manager while the window is open;
- for the extension selected, everything the manifest declares — identifier,
  version, author, licence, capabilities, whether it processes audio, the
  directory it is in, the size of its module, and each parameter with its kind,
  range and default.

Two buttons change something.

**Turn off / Turn on** moves the directory between the two slots. That is the
only change the manager makes to a disk, and it is the one described above.

**Verify** asks the extension host whether the extension really is one. The
listing can only say "this directory has a manifest and a module"; a module
built against another API version, a truncated `.wasm`, an `rx_init` that traps,
an `rx_process` that never returns — none of those are visible in a listing, and
all of them are what a user wants to find out. So Verify loads the extension with
the same host the editor links, exactly as the editor would (manifest rules,
module compiled, start-up run), and then pushes one block of silence through it.
Verification happens when the button is pressed and not before, because both
stages run the extension's code; a module that loops forever costs the host's
whole deadline before the check gives up, which is why that wait is something the
user asks for rather than something reading the folder does.

The window is egui, drawn with OpenGL: a native window, no browser and no
server. It is one process, and the directory work is in it — the same
`rustacity-ext-manager` library the listing program uses, called directly. egui
and eframe are MIT or Apache-2.0, so none of this changes the licence position of
the GPL-2.0-or-later application.

## Building and running it

```
cd rust
cargo run -p rustacity-ext-manager-gui     # the window
cargo run -p rustacity-ext-manager         # the listing, in a terminal
```

Both need a Rust toolchain and nothing else; the window also needs a working
OpenGL driver, which every desktop has. `cargo build --release` then puts the two
programs in `rust/target/release/`.

The listing takes `--root <dir>`, `--print-root`, `--help` and `--version`, and
with no arguments prints the same grouping the window shows. It changes nothing
on disk: turning extensions on and off is the window's work.

## What it does not do

- **It does not install or uninstall anything, and it never deletes.** Installing
  is copying a directory in; uninstalling is taking it away. A rename is the only
  write the manager makes.
- **It does not make an extension usable in the editor.** No menu entry, no
  parameter dialog: that is the editor-side work, and it is not started.
- **It does not set parameters.** The window shows what an extension declares and
  what the host will offer it; the editor has no dialog for parameters either
  (§4 of [INTEGRATION_NOTES.md](INTEGRATION_NOTES.md)).
- **It does not decide whether an extension is any good.** Verify says that it
  loads and runs, not that it is correct or safe to use, and it is not a
  substitute for running the extension's own tests.
- **It does not fetch, sign or verify signatures.** Installer signing is deferred
  in the editor too (§4 of [INTEGRATION_NOTES.md](INTEGRATION_NOTES.md)).
- **It does not move an extension whose name is already taken on the other
  side**, or follow a name that tries to leave the root. Both are refused, with
  the reason in the window.

## How it is checked

`rust/ext-manager` is the library and `rust/ext-manager-gui` is the window, and
CI builds both and runs their tests on every push. The tests cover the directory
and the window's own state changes:

- the scans, the two slots, and what counts as an extension at all;
- the move in both directions, the refusal when the name is taken on the far
  side, and the refusal of anything that is not a plain name;
- verification of a directory that claims to be an extension and has nothing to
  load, which is the case a listing cannot see;
- what the listing prints, given an inventory;
- the click handlers of the window — turning one off moves the directory, the
  listing is read again, the message says what happened, and the detail pane
  follows the extension to its new slot — driven without a window, so it runs
  anywhere.

What is *not* verified by a test is the window itself: that it opens, lays out
and repaints as intended was checked by running it against the fixtures in
`rust/extensions/dist/`, and it is not part of the automated checks.

## Why a separate program

The editor's own dialogs are wxWidgets C++, so a manager inside the editor means
C++ for the interface, a full application build to see it, and the whole
application's build to change it. The directory work is not like that: it is
`std::fs` and the extension host, which is what this repository's Rust crates
already are. Writing it as a crate keeps it testable with `cargo test` on every
platform CI runs on, and leaves the editor with the one job only the editor can
do — discovering what is installed, putting it in a menu, and offering the
parameters.
