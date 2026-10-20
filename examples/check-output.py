#!/usr/bin/env python3
# Rustacity: A Digital Audio Editor
# Copyright (C) 2026 Sato
# SPDX-License-Identifier: GPL-2.0-or-later
"""Check what the gain-invert extension did to a WAV, without trusting it.

This is the arithmetic done outside the extension, so a run through the CLI can
be checked rather than believed. The expectation is worked out here - input
times 10^(dB/20), negated if the polarity was flipped - and compared with the
output sample by sample.

    python3 examples/check-output.py examples/sine-sweep.wav out.wav --gain-db 6
    python3 examples/check-output.py examples/sine-sweep.wav out.wav --gain-db 0 --invert

It exits 0 when every sample is within 1e-6 of the expectation and 1 otherwise,
and prints the largest error either way. It reads the RIFF chunks itself
because the CLI writes 32-bit float in the WAVE_FORMAT_EXTENSIBLE form (format
tag 0xFFFE, sub-format IEEE float), which Python's own `wave` module refuses;
a 16-bit PCM input is read the same way. No third-party modules are needed.
"""

import argparse
import struct
import sys

TOLERANCE = 1e-6

PCM = 1
IEEE_FLOAT = 3


def read_wav(path):
    """Return (samples, channels, sample_rate, bits, format_tag)."""
    with open(path, "rb") as handle:
        data = handle.read()
    if data[0:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise SystemExit(f"{path}: not a RIFF/WAVE file")
    fmt = None
    payload = None
    position = 12
    while position + 8 <= len(data):
        chunk = data[position:position + 4]
        size = struct.unpack_from("<I", data, position + 4)[0]
        body = data[position + 8:position + 8 + size]
        if chunk == b"fmt ":
            tag, channels, rate, _, _, bits = struct.unpack_from("<HHIIHH", body, 0)
            if tag == 0xFFFE:  # extensible: the real format is in the sub-format
                tag = struct.unpack_from("<H", body, 24)[0]
            fmt = (tag, channels, rate, bits)
        elif chunk == b"data":
            payload = body
        position += 8 + size + (size & 1)
    if fmt is None or payload is None:
        raise SystemExit(f"{path}: no fmt or data chunk")
    tag, channels, rate, bits = fmt
    if tag == IEEE_FLOAT and bits == 32:
        samples = list(struct.unpack("<%df" % (len(payload) // 4), payload))
    elif tag == PCM and bits == 16:
        samples = [v / 32768.0
                   for v in struct.unpack("<%dh" % (len(payload) // 2), payload)]
    else:
        raise SystemExit(f"{path}: unhandled format tag {tag}, {bits} bits")
    return samples, channels, rate, bits, tag


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("input")
    parser.add_argument("output")
    parser.add_argument("--gain-db", type=float, required=True)
    parser.add_argument("--invert", action="store_true")
    arguments = parser.parse_args()

    expected_gain = 10.0 ** (arguments.gain_db / 20.0)
    if arguments.invert:
        expected_gain = -expected_gain

    source, channels, rate, bits, tag = read_wav(arguments.input)
    result, out_channels, out_rate, out_bits, out_tag = read_wav(arguments.output)
    print(f"input : {len(source)} frames, {channels} ch, {rate} Hz, "
          f"{bits}-bit, format tag {tag}")
    print(f"output: {len(result)} frames, {out_channels} ch, {out_rate} Hz, "
          f"{out_bits}-bit, format tag {out_tag}")
    if len(source) != len(result):
        print(f"FAIL: {len(source)} frames in, {len(result)} out")
        return 1
    if channels != out_channels:
        print(f"FAIL: {channels} channels in, {out_channels} out")
        return 1

    expected = f"{arguments.gain_db} dB" + (" with polarity flipped" if arguments.invert else "")
    print(f"expected: input x {expected_gain:.9f} ({expected})")
    worst = 0.0
    for a, b in zip(source, result):
        worst = max(worst, abs(b - a * expected_gain))
    print(f"largest error: {worst:.3e} (tolerance {TOLERANCE:.0e})")
    if worst <= TOLERANCE:
        print("PASS")
        return 0
    print("FAIL")
    return 1


if __name__ == "__main__":
    sys.exit(main())
