# Repository layout

Rustacity is a fork of Audacity 2.4.1. The application is still the upstream
C++ tree; its audio maths is being replaced, unit by unit, by the Rust core in
`rust/`. This page says what every top-level entry is, which parts are ours,
and which parts are inherited and must not be touched.

## Top level

| entry | what it is | in the build |
| --- | --- | --- |
| `src/` | the C++ application: the wxWidgets shell, the effect classes, the project and track model, the file formats | yes - `add_subdirectory( src )`, and `src/CMakeLists.txt` runs `cargo` and links the Rust cores |
| `rust/` | the Rust DSP core, crate `rustacity-dsp`, and the sandboxed extension engine, crate `rustacity-ext-host`, with the extensions themselves in `rust/extensions/` (see [`docs/architecture.md`](architecture.md)) | yes - built by cargo, linked as static libraries |
| `include/` | headers shared across `src/` | yes |
| `lib-src/` | vendored third-party libraries (portaudio, libsndfile, LV2, libvorbis, ...) plus `lib-src/FileDialog/` | yes, through `cmake-proxies/` |
| `cmake-proxies/` | the CMake project that builds each vendored library, plus the project's own CMake modules | yes |
| `help/` | help pages and manual assets installed with the application | yes |
| `images/` | brand assets and theme XPMs; `images/brand/` holds the SVG sources | yes |
| `locale/` | application translations: the `.po` catalogs, `LINGUAS` and `LanguageNames.txt` | yes |
| `nyquist/` | the Nyquist language runtime resources and system plug-ins | yes |
| `plug-ins/` | bundled Nyquist plug-ins | yes |
| `presets/` | shipped preset data (`EQDefaultCurves.xml`) | yes |
| `scripts/` | our tooling - `scripts/brand/` (brand asset generation) and `scripts/check_dsp_abi.py` (the ABI gate) - alongside inherited wiki and image utilities | partly |
| `win/` | the Windows installers (`Inno_Setup_Wizard/` for the wizard, `wix/` for the MSI), the application resource script and the icon it embeds | yes for `rustacity.rc` and the icon; both installers are built in CI |
| `mac/` | macOS bundle resources (`Resources/`), the bundle wrapper `Wrapper.c`, `Info.plist`, signing entitlements, and the local wxWidgets build helper | yes for `Wrapper.c` and `Resources/` |
| `.github/` | CI - one workflow, `cmake_build.yml` | yes |
| `docs/` | this documentation | no |

At the root, `CMakeLists.txt` is the build entry point. `README.md` is the
front page, `LICENSE.txt` the GPL (with the upstream attribution intact),
`CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` and `PULL_REQUEST_TEMPLATE.md` the
inherited GitHub meta-files.

## What is ours

- `src/`, and everything the Rust migration touches inside it.
- `rust/`.
- `scripts/brand/` and `scripts/check_dsp_abi.py`.
- `docs/`, `README.md`, `.github/workflows/cmake_build.yml`.
- The brand assets in `images/` and the icons and installer art in `win/` and
  `mac/Resources/`, all generated from `images/brand/` by
  `scripts/brand/build_brand_assets.py`.

Everything else is inherited from Audacity 2.4.1 or vendored third-party code.
`lib-src/` is vendored: it is never edited to make our code fit, and never
restructured.

## Why GitHub reports so little Rust

GitHub counts bytes of code per language on the default branch. Before the
cleanup below its numbers were: C 36%, C++ 22%, Python 14%, Shell 12%,
Makefile 8%, M4 1.3%, Common Lisp 1.3%, and **Rust 0.22%**.

Those numbers are accurate, not stale, and they are a poor measure of this
project. When the extension engine landed the same report said: C 37.6%, C++
21.5%, Python 15.0%, Shell 11.7%, Makefile 6.1%, and **Rust 0.81%** - 440 KB of
the 54 MB it counts. `src/` is about 318,000 lines of C++; `rust/` is about
13,000 lines of Rust, of which about 5,400 are the DSP core (`rust/dsp/src`)
and the rest the extension engine, its tests and the example extensions. The C
and Python and Makefile shares are vendored `lib-src/` and build scaffolding,
which is why they are worth being able to tell apart - hence this page. The
only metric that tracks progress honestly is the ledger in
[`docs/port-status.md`](port-status.md).

## Inherited infrastructure that was removed

The repository used to carry two CI systems, an entire second build system and
the IDE projects for both desktop platforms, none of which any command we run
referenced. It was removed in six commits, each of them checked with `git
grep` across the CMake tree and the workflow before anything was deleted:

| commit | what went |
| --- | --- |
| `chore: retire the unused CI configurations` | `.github/workflows/build.yml` (dormant: it triggered only on a branch named `disable`), `.travis.yml`, `appveyor.yml` |
| `chore: drop the autotools build` | `configure`, `configure.ac`, `Makefile.am`/`Makefile.in`, `autotools/`, `m4/`, `INSTALL`, `ABOUT-NLS`, `scripts/maketarball.sh`, `locale/Makefile.in.in`, `nyq-po/`, the `po` symlink to `locale/` |
| `chore: drop the unused doxygen config and QA scripts` | `rustacity.dox`, `dox2-src/`, `qa/` |
| `chore: drop the autotools-only tests and upstream notes` | `tests/` (built and run by autotools only - the CMake build never compiled it), `todo.txt`, upstream `README.txt` and `CHANGELOG.txt` |
| `chore: retire the Visual Studio and Xcode projects` | `win/rustacity.sln`, `win/Projects/`, `win/wxWidgets_additions/`, the MSBuild props/targets/rules, `Directory.Build.props`, `mac/Rustacity.xcodeproj/`, `mac/wxMac_additions/`, `mac/config/`, the xcconfigs, and the Xcode/dmg packaging scripts |
| `chore: drop the files the build never read` | `win/LADSPA_plugins-win/` (a prebuilt plug-in set that nothing packaged), `win/darkaudacity.ico`, `scripts/maketarball.cmake` with the `minsrc` target that ran it, `mac/cmake_build.txt` |

The CMake build is the only build. It generates its own Xcode project and
Visual Studio solution, it drives the translations from `locale/` itself, and
the Windows installers are made from its output directory: the wizard in
`win/Inno_Setup_Wizard/` and the MSI in `win/wix/`, both packaging the tree
the 7z holds. The release's sources tarball is packed by the workflow, with
`git archive`.

## Is a file still load-bearing?

Before deleting anything, check whether the build names it:

```sh
# everything the CMake project reads, plus the workflow
git grep -n "<name>" -- CMakeLists.txt cmake-proxies src/*/CMakeLists.txt \
    .github/workflows
```

A path that appears nowhere but in the file being considered, and in
`.gitignore`, is dead weight of the kind listed in the table above.
