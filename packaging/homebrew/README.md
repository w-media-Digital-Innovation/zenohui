# Homebrew packaging (local preparation)

Nothing in this directory creates commits, pushes, creates a GitHub repository,
or publishes a release. All such actions require explicit user approval.

Publish and verify the stable zenohui release first. Then download its **source**
archive, not a prebuilt binary archive, and generate the tap formula from those
exact bytes:

```bash
python3 packaging/homebrew/prepare.py \
  --archive /path/to/zenohui-0.2.0-source.tar.gz \
  --url https://github.com/w-media-Digital-Innovation/zenohui/archive/refs/tags/v0.2.0.tar.gz
```

The supplied archive must be the file downloaded from the supplied URL. The
script reads its package version, calculates SHA-256 and writes
`target/homebrew/Formula/zenohui.rb`. It does not download or publish anything.
The formula builds from locked sources and installs the executable, shell
completions and manpages. Its functional test uses a local listener and a dry run.
It targets Homebrew 7's separate Cargo fetch/build phases.

For unpublished working files only:

```bash
python3 packaging/homebrew/prepare.py --local
```

This creates a private local source snapshot and a formula with a `file://` URL
under `target/homebrew/`. Do not distribute it or use its checksum for a release.

Homebrew requires the generated formula to be placed in a registered tap before
installation. After the release, and only with explicit approval, create the tap,
place the generated formula in `Formula/zenohui.rb`, then run:

```bash
brew style w-media-Digital-Innovation/tap/zenohui
brew install --build-from-source w-media-Digital-Innovation/tap/zenohui
brew test w-media-Digital-Innovation/tap/zenohui
brew audit --strict w-media-Digital-Innovation/tap/zenohui
```

The installation normally obtains Rust as a build dependency. No tap has been
registered and no installation has been completed by this preparation.
