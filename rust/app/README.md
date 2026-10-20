# Rustacity desktop application

This workspace is the iced shell that replaces the wxWidgets application in
`src/`. It is deliberately separate from `rust/`: the GUI stack is not part of
the C++ build or the DSP test suite.

## Run

```sh
cd rust/app
cargo run -p rustacity-app
```

M0 opens a window with the shell layout and no audio yet. M1 adds the first
vertical slice: open a WAV, draw its waveform and play it.

## Layout

| Crate | Role |
| --- | --- |
| `rustacity-app` | binary, state, update loop |
| `rustacity-ui` | views and widgets |
| `rustacity-engine` | project, tracks, transport, edit operations |
| `rustacity-io` | devices and file formats |
| `rustacity-project` | `.aup` and blockfile persistence |

## Gates

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The DSP core stays in `rust/dsp`; this workspace depends on it by path.
