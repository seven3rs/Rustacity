# Writing an extension

An extension is a WebAssembly module and a manifest, described in
[API.md](API.md), and run in the sandbox described in [SANDBOX.md](SANDBOX.md).
This is the practical version: what to write, in what order, and how to see it
work. Nothing here needs the Rustacity tree — an extension is a project of its
own — but this tree has three you can read:

- `rust/extensions/template` — a starter, with the parts to fill in marked;
- `rust/extensions/gain-invert` — the same thing finished, and the one the
  host's own tests measure against;
- `rust/extensions/hostile` — ten ways to misbehave, and what the sandbox does
  about each.

## 1. What you need

- A Rust toolchain with the WebAssembly target:

  ```
  rustup target add wasm32-wasip1
  ```

  `wasm32-wasip1` is the WebAssembly System Interface, preview 1. The sandbox
  links it but leaves it empty, which is why a `std`-built module works: nothing
  it asks for will succeed, but nothing it asks for is missing either.

- Any language that can produce a WebAssembly module and follow the ABI. Rust is
  what the examples and the tooling are in; the ABI is eight exported functions
  and one import, and it is written down in [API.md](API.md).

## 2. Start from the template

```
cp -r rust/extensions/template my-effect
```

Then:

1. **Rename the crate** in `my-effect/Cargo.toml`: `name`, and the `[lib] name`
   under it (which is the `.wasm` file's name in your build directory — the
   installed file is `extension.wasm` regardless).
2. **Write `my-effect/extension.toml`**: at least a new `id` (reverse-DNS, and
   the unique name of your extension), `name` (what the user sees), and
   `version`.
3. **Replace the parameter** in the manifest with your own, and the matching
   `PARAM_*` constant and `match` arm in `rx_set_param`.
4. **Write `rx_process`.** That is the extension.

If you also want to build it from this tree, add a line to
`rust/extensions/variants.txt` and run `scripts/build-extensions.sh`; that is
how the fixtures under `rust/extensions/dist` are made.

## 3. The manifest

```toml
api_version = 1
id = "com.example.my-effect"        # unique, reverse-DNS, never changes
name = "My Effect"                  # the user sees this
version = "0.1.0"                   # your extension's version
description = "One line, in the user's language"
author = "Your Name"
license = "GPL-2.0-or-later"

capabilities = ["audio.read", "audio.write"]

[[parameters]]
id = "amount"
name = "Amount"
kind = "float"
min = 0.0
max = 1.0
default = 0.0
description = "Shown as the parameter's tooltip"
```

Everything is checked before your module is even compiled: a misspelled field, a
`default` outside its own range, a duplicate parameter id, an id that is not
reverse-DNS, an unknown capability — each one is a load failure naming the field.
The rows are in [API.md, section 2](API.md#2-the-manifest), and the host will not
run an extension that fails them.

Parameters are in the manifest's order, and that order is the index your
`rx_set_param` is called with. Adding a parameter at the end is safe; inserting
one in the middle changes the meaning of every index after it, so do it in a
version bump at least.

`capabilities` is a declaration, not a restriction: `audio.read` and
`audio.write` are required for an effect that is handed audio, and anything else
is refused, because version 1 has nothing else to offer.

## 4. The module

Eight exports, checked with their exact signatures at load. `template/src/lib.rs`
has all of them; this is what each must do.

| export | must do |
| --- | --- |
| `rx_api_version` | return 1 |
| `rx_alloc(size)` | return a pointer to that many bytes, aligned for `f32`, or 0 |
| `rx_free(ptr, size)` | release exactly what `rx_alloc` returned, with the same size |
| `rx_init(sample_rate, max_block_frames, channels)` | remember these three, reset your state, return 0 |
| `rx_set_param(index, value)` | take a parameter, in the manifest's order |
| `rx_process(in_ptr, out_ptr, frames)` | process `frames` frames, return 0 |
| `rx_reset()` | forget what you were doing; optional |

Two rules cover most of the mistakes:

- **The audio is planar.** `in_ptr` and `out_ptr` each point at `channels`
  consecutive blocks of up to `max_block_frames` `f32` samples: channel `c`'s
  sample `i` is at `c * frames + i`. Interleaving here is the classic bug, and
  it sounds like a rattle.
- **`frames` is not a constant.** It is 1 or more and never more than
  `max_block_frames`, and the last call of a track is usually short. Loops must
  use `frames`, not a remembered block size.

You may read `in_ptr` and write `out_ptr`, or write the answer over `in_ptr` and
ignore `out_ptr`; the host copies `out_ptr` back afterwards either way. Do not
keep either pointer: the host owns the buffers and frees them through `rx_free`
after the last block.

### Return codes

`rx_init` and `rx_process` return 0 for success and non-zero for failure, and
your number is reported to the user as it is. Check the two things that can
actually be wrong — that you were initialized, and that `frames` is in range —
and return your own code, as the template does, rather than letting a panic
happen: a panic is a trap, the sandbox contains it, but the user has lost the
operation and gets a message about a trap rather than about what you refused.

### State

`rx_init` is where state starts. The host calls it once, before the first block,
so anything you keep between calls belongs in a module-level `static` — in Rust,
`static mut` with `addr_of_mut!`, as the template does, because the host never
calls two of your exports at the same time. That last part is the whole of the
thread-safety story: no threads exist on either side of the boundary, so the
only thing to be careful of is re-entering yourself.

The parameter values you are given have already been checked against the
manifest: a float is within its `min`/`max`, a switch is 0.0 or 1.0, a choice is
a valid index. A value that does not pass is refused before the call, and never
reaches you.

### Changing a parameter while it runs

`rx_set_param` may arrive after the audio has started, and a gain that jumps
between two blocks is a click. The shape that avoids it, from `gain-invert`:

- set before the first block: take the value exactly, so a fresh run is the
  arithmetic the user asked for;
- set after that: move to it over a fixed number of frames (128 in
  `gain-invert`), linearly;
- `rx_reset`: forget that processing started, so the next value is taken at once
  again — the host calls it when it seeks or restarts.

When you do keep a per-frame value like that, **step it once per frame, not once
per sample**: inside a channel loop, the second channel starts part way through
the curve and the channels come out at different gains, which is a stereo image
sliding sideways for the length of the change. `gain-invert`'s `rx_process` is
the loop shape that gets this right.

## 5. Logging

The host provides one import. It is worth using while you develop:

```rust
#[link(wasm_import_module = "rustacity")]
extern "C" {
    fn rx_log(level: i32, ptr: i32, len: i32);
}
```

`level` is 0 trace, 1 debug, 2 info, 3 warn, 4 error. Write your message to a
`&[u8]` and pass its pointer and length. The host rate-limits this to 64 messages
and 8 KiB per call and truncates a single message at 512 bytes; going over costs
you messages, never the call. `rustacity-ext-cli --verbose` prints what was
logged.

## 6. Building and trying it

```
cargo build --release --target wasm32-wasip1
```

The module lands in `target/wasm32-wasip1/release/<your lib name>.wasm`. An
extension is a directory holding exactly two files, named for what they are:

```
my-effect/
  extension.toml
  extension.wasm      <- your built module, under this name
```

Run it without the editor:

```
rustacity-ext-cli --describe my-effect
rustacity-ext-cli my-effect in.wav out.wav --param amount=0.5 --verbose
```

`--describe` prints the manifest as the user will meet it — name, id,
capabilities, each parameter with its range and default — which is the quickest
way to see that the manifest says what you meant. The run writes 32-bit float
WAV whatever the input was, in blocks of `--block` frames (4096 by default), one
instance for the whole file, so what you hear is what the editor will get.

If the output is silence, check that `rx_init` returned 0 and that `rx_process`
is using `frames` rather than `max_block_frames`; if it is a rattle, check the
`c * frames + i` indexing.

Where the editor installs extensions from, and how it lets a user turn one on
and off, is part of the editor-side integration and is not settled here yet.

## 7. Testing it

The host's tests are the model: compare against a reference the test works out
for itself, not against your own code. `rust/ext-host/tests/gain_invert.rs` does
that for a multiply, at 1e-6, over several channel counts, gaining and inverting;
`block_sizes.rs` runs the same input at 1, 64, 4096 and 65536 frames per call and
insists the output does not depend on the blocking. Both are worth copying in
spirit even if your extension is not in this tree:

- the same input in different block sizes gives the same samples (it catches
  state that is stepped per block rather than per frame);
- a block of one frame works;
- the last block may be short;
- a parameter at each end of its range works, and one just outside it is
  refused before you see it.

## 8. Checklist

- [ ] `extension.toml`: `api_version = 1`, a reverse-DNS `id`, a `name`, a
      `version`, and the capabilities you actually use.
- [ ] Every parameter has an id, a kind, and a default inside its range; the
      indices in `rx_set_param` match the manifest's order.
- [ ] All eight exports present with the exact signatures.
- [ ] `rx_process` uses `frames`, indexes `c * frames + i`, and never reads or
      writes outside the two buffers.
- [ ] Nothing panics on a bad argument; you return a code instead.
- [ ] `rustacity-ext-cli --describe` shows what you intended, and a run through
      it sounds right.
- [ ] The output does not change between block sizes.
