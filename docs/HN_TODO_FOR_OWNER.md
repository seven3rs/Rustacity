# For the owner: what this branch could not do, and what to decide

Everything here needs either your account, your judgement, or a piece of hardware
this machine does not have. Nothing in it blocks the branch; the first group is
worth doing before the branch is merged, the second before anything is posted.

## Before merging

- [ ] **Set the repository description and topics.** `gh` is not installed here,
      so these are the commands to run:
      ```sh
      gh repo edit prantosmss/Rustacity \
        --description "An Audacity 2.4.1 fork: sandboxed WebAssembly extensions, and audio DSP moving into Rust"
      gh repo edit prantosmss/Rustacity \
        --add-topic rust --add-topic audio --add-topic dsp --add-topic webassembly \
        --add-topic wasm --add-topic audacity --add-topic audio-editor --add-topic sandbox \
        --remove-topic rustacean --remove-topic rustaceans
      ```
      The two removed topics are not things anybody searches for, and the
      missing ones are.
- [ ] **Check the licence is detected as GPL-2.0 or later** on the repository
      page once this branch is pushed. It said `NOASSERTION` before, because the
      fork notice was prepended to the GPL text; `LICENSE.txt` is now upstream's
      file byte for byte, with the fork recorded in `NOTICE`. If it still says
      `NOASSERTION`, the reason is somewhere else and I would like to know.
- [ ] **Decide the "v2 only or v2 or later" question.** `NOTICE`, `README.md`
      and the three `Cargo.toml` files say GPL-2.0-or-later; the notice kept in
      upstream's `LICENSE.txt` says "GNU GPL Version 2". Either reading can be
      argued, so it is yours to settle — and it is one word in four places. I am
      not a lawyer and did not change the licence text.
- [ ] **Turn on private vulnerability reporting** (repository Settings → Security)
      if you want `SECURITY.md` to be able to send people to the *Report a
      vulnerability* form. If not, `SECURITY.md` lists the email address that is
      already in 19 files in this tree; edit it if you would rather not use it.
- [ ] **Check the new `cli` job's first real run.** It is not tag-only, so the
      push that lands this branch exercises everything except the release
      mapping: it builds the CLI and the twelve extensions, runs the quick start
      on the committed sample, and compares the generated WAV with the file in
      the tree. The release job's new `*.tar.gz` line and its `SHA256SUMS` step
      can only run on a tag, so the **next tag is the first run of those two** —
      watch it, and if it fails, nothing else in the release has been broken.
- [ ] **A trap found while writing that job, not caused by it:** the `goldens`
      job uploads its captured values as an artifact, and the release job fails
      on any `dist/` file it has no name for. Today the goldens job does nothing
      (no dumpers in the tree), so no release is affected. The day a port adds a
      dumper, the next release will fail at the asset-renaming step until the
      release job is told to ignore `golden-values`.

## Before posting anywhere

- [ ] **Answer the two placeholders.** Both are marked `TODO(owner)` in the text
      and neither is mine to invent:
      - why the fork is based on 2.4.1 rather than a 3.x release — it is not
        written down anywhere in the tree (`README.md`, "Relationship to
        Audacity"; `docs/hn/FAQ_FOR_COMMENTS.md`);
      - what, if anything, to say about how the code was written
        (`docs/hn/FAQ_FOR_COMMENTS.md`, last third) — it is the one claim a
        reader cannot check against the tree, which is exactly why it should be
        your sentence and not mine.
- [ ] **Decide when the README stops leading with the CLI.** It currently says
      the releases exist and that they cannot run an extension yet, then sends
      the reader to the CLI. Once a release contains the menu entry (see below),
      that paragraph is the one to rewrite, and `docs/HN_AUDIT.md` §2 says where
      the "was it built here" sentence lives.
- [ ] **Have a build to point at.** Either the current release plus the honest
      "cannot run an extension yet", or a 0.6.0 that contains the engine wired
      in. Unsigned is fine: 0.5.0 is unsigned too, and both platforms' warnings
      are written down as a known limit.
- [ ] **Screenshots or a short screen recording** — needs a display and a built
      application, so it could not be made here. The three worth having, in
      order: the extension doing something visible in the editor (**does not
      exist yet** — see the next item); `gain-invert` in the CLI with the
      checker's output beside it; the hostile fixture's log. If only the second
      and third exist, say so in the post rather than implying the first.
- [ ] **Read `docs/hn/SHOW_HN_DRAFT.md` before it goes anywhere.** It is a draft:
      the title options, the tone, and the "looking for feedback" paragraph are
      yours to change, and the first comment should be re-read against whatever
      the repository looks like on the day.

## Decisions about the code, which are larger than this branch

- [ ] **Whether to add the GUI entry point before the next tag.** Nothing
      constructs `EffectExtension` today, so an extension cannot be run from the
      application. The smallest version of the work that changes that is
      discovery over a directory of extension folders, a menu entry per
      manifest, and a parameter dialog generated from the manifest's rows —
      roughly the shape `src/effects/Extension.{h,cpp}` was written for. Until
      it lands, the CLI is the whole story, and both the README and the FAQ say
      so plainly.
- [ ] **Whether to re-translate the four help strings this branch changed.**
      `src/HelpText.cpp` now says Rustacity where it said Audacity and points at
      this repository's issues instead of calling the upstream forum "our"
      support. Those four messages fall back to English in existing locale
      files until `xgettext`/the translation update is run.
- [ ] **What to do about the inherited `#ifdef EXPERIMENTAL_DA` code.** It is
      inert (`src/Experimental.h` has the switch commented out), it is six files
      of DarkAudacity strings, and removing it is a large diff across inherited
      files. `docs/HN_AUDIT.md` records why it was left alone; the alternative is
      a cleanup branch of its own.
- [ ] **Two merged branches and a stale `master`:**
      ```sh
      git push origin --delete dsp/abi-hardening dsp/phaser
      git branch -d dsp/abi-hardening dsp/phaser
      git branch -D master      # only if it is the stale local one
      ```
      Not done here: deleting refs is not something this branch should decide,
      and `master` in particular may be a name you want to keep.
- [ ] **The help-text edit could not be compiled here** (no C++ toolchain). It is
      a string-literal swap in one file, and CI is the compiler of record — but
      it is the one change in this branch that a build has not yet seen.

## Checked, and needing nothing

No secrets in the tree, no committed build output, no file over 1 MB, no
`README.txt` duplication, no dead CI files, no second front page, no telemetry
and no network client at all, and the workflow's three triggers are unchanged. The
full detail, with the commands, is `docs/HN_AUDIT.md` §4.
