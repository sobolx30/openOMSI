# Contributing to openOMSI

Thanks for helping! A few rules keep the project healthy:

* **No original code or assets.** Never copy anything from an OMSI 2 installation into the
  repository - no textures, models, sounds, maps, scripts. Tests that need
  content read it from a local installation (`OMSI_ROOT`) and skip themselves without one.
* **Compatibility first.** A change must not break a stock map or a mod that worked. Run
  `cargo run --release -p omsi-check -- "/path/to/OMSI 2"` before and after larger changes.
* **One change per pull request**, with a message that says what the player notices.
* `cargo test --workspace` and `cargo build --release` must pass (CI checks all platforms).
* Code style: `rustfmt` defaults, comments explain *why*.

## Files and licences

* **Contributions fall under [LICENSE](LICENSE)**: by sending one you agree that it becomes
  part of the project's modifications (Part 2 of the licence: all rights reserved, usable
  only as part of this program). The few
  third-party files the game ships (the Roboto fonts, the Material icons with their
  `LICENSE.txt`) keep their own licences; adding another - map data from OpenStreetMap
  (ODbL), CC-BY-SA pictures, other fonts or icon sets - needs the maintainers' agreement
  first. A format can be documented here and its real data published elsewhere (a release,
  a repository of its own, the mod itself).
* **No large files.** A file of a megabyte or more fails the PR checks. Screenshots and
  videos belong in the pull request's description, not in a commit; test data is small and
  made for the test.
* **No generated or downloaded content**: no builds, caches, converted textures or example
  maps.

## Translations

The interface texts are in [`crates/omsi-app/locales/app.yml`](crates/omsi-app/locales/app.yml),
with the English text as the key. A pull request that changes it is checked automatically:

* add or correct texts of your language; keep the placeholders (`%{packs}`, `{}`) of the key;
* do not remove a key or reword the English one - the code looks texts up by it;
* changing another contributor's existing translation is fine when it is a correction - say
  so in the description, the check lists every changed text for the reviewers.

## Issues

* **English only** - titles and text, so every contributor can read and search them. An
  issue in another language is closed automatically with a request to translate it, and
  opens again by itself once it is edited into English. Logs and game text can stay as
  they are.
* One problem or idea per issue, on the latest release. Questions go to the
  [Discord server](https://discord.gg/FQpVKtVJxD).
* New issues are sorted automatically: the form's "What is it about?" becomes an `area:`
  label and the issue gets a first milestone (a regression or a crash: v0.1.x), which the
  maintainers move when it fits better elsewhere. A crash report with the same panic as an
  open issue is closed as its duplicate - add what you were doing to that issue instead.
* **Security problems are not reported as issues**: see [SECURITY.md](SECURITY.md).

## Where things are

See the layout in the [README](README.md#repository-layout) and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). File formats: [docs/FORMATS.md](docs/FORMATS.md).

## Releases

Maintainers bump `MAJOR.MINOR` in the `VERSION` file; everything else is automatic - see
[docs/VERSIONING.md](docs/VERSIONING.md).
