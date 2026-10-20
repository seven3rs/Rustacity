# Security policy

## Reporting a problem

Please report privately, not in a public issue: use **Security → Report a
vulnerability** on this repository's GitHub page (GitHub's private
vulnerability reporting), or email <seven3rs@gmail.com>. `TODO(owner)`:
confirm the address to use for security reports, and enable private
vulnerability reporting on the repository if that is the route you prefer.

Include what you found, how to reproduce it, which part it is (the editor and
the C++ tree, the Rust DSP core, or the extension engine and its sandbox), and
the version or commit. Please do not include a working exploit against someone
else's installation.

## What is in scope

- **A sandbox escape**: anything that lets an extension read or write files,
  reach the network, see the environment or the project, or affect another
  instance, beyond what [`docs/extensions/SANDBOX.md`](docs/extensions/SANDBOX.md)
  describes.
- **Containment failures**: a hostile or broken module taking the editor down,
  or leaving it unusable, instead of failing its own invocation.
- **Memory-safety or panic bugs in the Rust core** reachable from audio input or
  from an extension.
- **Supply-chain problems**: a release asset, a build input or a dependency that
  is not what the source says it is.

## Known limits, which are not vulnerabilities

The sandbox is **not an operating-system boundary**. An extension runs in
Rustacity's own address space, inside Wasmtime, so a bug in Wasmtime or in the
host's own glue is a bug in the editor; running extensions in a separate process
is not built yet ([`docs/extensions/SANDBOX.md`](docs/extensions/SANDBOX.md) §3).
Reports about that design are welcome as ordinary issues.

Legacy plug-in formats — VST, LV2, AU, Nyquist — are native code and are **not**
sandboxed by this project. A problem in one of them is a problem in that plug-in
or in upstream Audacity 2.4.1's code; we would still want to know about it.

Since 1.0 does not exist yet, an interface change is not a security matter
either.

## What to expect

`TODO(owner)`: state here the response time you are willing to commit to, and
whether fixes will be backported to a released version or land on `main` only.
Until then: this is a small project maintained by one person, reports are read
and answered as quickly as they can be, and a fix will be credited in the
release notes unless you ask otherwise.
