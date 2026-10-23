# Quality checks before committing

## Required local gate

Install Python 3 and rustup, then run from the repository:

```bash
python3 scripts/check.py
```

`rust-toolchain.toml` pins Rust, rustfmt and Clippy to **1.93.1**. The same
command runs on Linux, macOS and Windows in GitHub Actions. Once dependencies
are cached, use `python3 scripts/check.py --offline`.

The gate stops at the first failure and checks:

- Whitespace errors in staged and unstaged diffs.
- Formatting with the pinned stable formatter.
- All-target Clippy, treating warnings as errors.
- All-target tests, including a temporary loopback-only Zenoh router that
  exercises router display, failed reconnect rollback and old-session cleanup.
- Documentation, treating documentation warnings as errors.

The checks do not stage files, create commits, push, tag or publish anything.
A successful local gate does not prove success on another operating system.
Review the diff and inspect the CI results for the exact pushed commit.

## CI and release checks

Required checks are the native quality gate on three operating systems,
minimum-Rust 1.92.0 tests, and coverage generation. Beta/nightly tests are
advisory and do not replace the pinned checks. Coverage is an inspection aid;
there is no arbitrary percentage threshold. The HTML report is uploaded as an
artifact so missing coverage can be reviewed.

Tagged release builds depend on all required checks. Release publishing also
waits for every configured packaging target to finish. These cross-compilation
and packaging jobs must still be observed in GitHub; a local macOS test does
not validate Linux packages or Windows archives.

## Focused manual check before v0.2.0

- Navigate all four tabs with keyboard and mouse.
- Open a JSON array, select a numeric leaf, inspect its history and graph.
- Apply a valid connection, then an invalid endpoint; confirm the old session
  remains usable after failure and the status message explains what happened.
- Save/load a profile, and verify malformed profiles leave the form unchanged.
- Resize the terminal and quit cleanly.
- Check README screenshots/examples for private addresses and other unintended
  data before publishing.

The external router test is explicitly ignored in the default suite. Run it
only against an endpoint you are authorised to use:

```bash
ZENOHUI_TEST_ROUTER=tcp/127.0.0.1:7447 \
  cargo test --locked remote_router_is_listed_without_multicast -- --ignored
```

## Homebrew preparation

See [packaging/homebrew/README.md](packaging/homebrew/README.md). A formula
syntax/style check alone is not an installation test. Source installation,
`brew test`, and audit against the actual published archive are separate checks.

## Approval

A request to improve or run quality checks does not authorise Git mutations.
Commits, pushes, tags, pull requests and releases require explicit user approval.
