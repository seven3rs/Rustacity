# Examples

A generated sample and two small scripts, so that the extension engine can be
tried and checked without building the editor.

| file | what it is |
| --- | --- |
| `sine-sweep.wav` | the sample: mono, 44.1 kHz, 16-bit, one second, 100 Hz to 8 kHz at half scale |
| `make-sine-sweep.py` | writes that file; standard library only, deterministic |
| `check-output.py` | checks what a gain-invert run did, sample by sample |

The WAV is generated, never recorded, so it carries no third-party audio and no
licence of its own.

## The sample

```
88,244 bytes
SHA-256  144f9ec395fd57901a95b29e10d9134ebc73d4989314827cec072eec0a4a81f9
```

Re-running the generator has to reproduce that file byte for byte:

```
python3 examples/make-sine-sweep.py /tmp/sine-sweep.wav
```

The sweep is ramped logarithmically, at half scale so that +6 dB does not clip;
it is deliberately a signal whose amplitude is easy to predict rather than
anything that sounds good.

## Run an extension over it

The extension has to be built first (`scripts/build-extensions.sh`, which needs
`rustup target add wasm32-wasip1`), and the CLI built too
(`cargo build --release -p rustacity-ext-cli`, from `rust/`).

```
rust/target/release/rustacity-ext-cli rust/extensions/dist/gain-invert \
    examples/sine-sweep.wav /tmp/gained.wav --param gain_db=6 --verbose
```

which prints, in full:

```
gain_db = 6
Gain / Invert: 44100 frame(s) of 1 channel(s) at 44100 Hz, 4096 frames per call
call    1:   4096 frames in 0.555 ms
call    2:   4096 frames in 0.525 ms
...
call   11:   3140 frames in 0.419 ms
44100 frame(s), 11 call(s) -> /tmp/gained.wav ('Gain / Invert' 0.1.0)
5.674 ms inside the extension, slowest call 0.612 ms
```

(The middle calls are elided; the millisecond figures are this machine's and
mean nothing beyond "the extension's own share of a one-second file is about
six milliseconds". No performance claim is made anywhere in this repository.)

The output is always 32-bit float, in the `WAVE_FORMAT_EXTENSIBLE` form (format
tag 0xFFFE with an IEEE-float sub-format) — every editor takes it, but Python's
`wave` module does not, which is why `check-output.py` reads the RIFF chunks
itself.

## Check the run

```
python3 examples/check-output.py examples/sine-sweep.wav /tmp/gained.wav --gain-db 6
```

```
input : 44100 frames, 1 ch, 44100 Hz, 16-bit, format tag 1
output: 44100 frames, 1 ch, 44100 Hz, 32-bit, format tag 3
expected: input x 1.995262315 (6.0 dB)
largest error: 6.399e-08 (tolerance 1e-06)
PASS
```

The same file at 0 dB and with the polarity flipped, from the two runs the
quick start makes:

```
$ python3 examples/check-output.py examples/sine-sweep.wav /tmp/identity.wav --gain-db 0
expected: input x 1.000000000 (0.0 dB)
largest error: 0.000e+00 (tolerance 1e-06)
PASS

$ python3 examples/check-output.py examples/sine-sweep.wav /tmp/flipped.wav --gain-db 0 --invert
expected: input x -1.000000000 (0.0 dB with polarity flipped)
largest error: 0.000e+00 (tolerance 1e-06)
PASS
```

Exact to the last bit at 0 dB, because the input's 16-bit values divide exactly
by 32768 and the extension multiplies them by 1; +6 dB is a single float
multiply, and 6.4e-08 is that multiply's own rounding. The parameters were set
before the first block, so the extension takes them exactly rather than slewing
to them (`docs/extensions/WRITING_AN_EXTENSION.md`, section 4).

## What a misbehaving extension looks like

`rust/extensions/hostile` builds one fixture per kind of misbehaviour. The one
that never returns is the interesting one from the outside:

```
$ rust/target/release/rustacity-ext-cli rust/extensions/dist/hostile-loop \
      examples/sine-sweep.wav /tmp/loop.wav
rustacity-ext-cli: the extension timed out: the extension was still working after 5000 ms
$ echo $?
1
```

That took 5.54 s of wall clock and left nothing running; the next command in the
same shell works normally. The fixture that goes looking for files, environment
variables, arguments and sockets logs what it found, and what it found is
nothing:

```
$ rust/target/release/rustacity-ext-cli rust/extensions/dist/hostile-wasi \
      examples/sine-sweep.wav /tmp/wasi.wav --verbose
...
[info] /etc/passwd: No such file or directory (os error 44)
[info] /etc/shadow: No such file or directory (os error 44)
[info] 0 arguments
[info] connect: operation not supported on this platform
[warn] obtained 0 bytes it should not have
44100 frame(s), 11 call(s) -> /tmp/wasi.wav ('Hostile (test fixture)' 0.1.0)
```

(The same block of messages is written once per call, eleven times; it is
elided here.) The full table of ten misbehaviours and what the sandbox did about
each is in [`docs/extensions/SANDBOX.md`](../docs/extensions/SANDBOX.md).

A parameter outside its range is refused before the extension sees it, never
clamped:

```
$ rust/target/release/rustacity-ext-cli rust/extensions/dist/gain-invert \
      examples/sine-sweep.wav /tmp/bad.wav --param gain_db=1000
rustacity-ext-cli: invalid argument: 1000 is outside the range of 'gain_db' (-60..24)
```
