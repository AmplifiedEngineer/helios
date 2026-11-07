<!-- SPDX-License-Identifier: Apache-2.0 -->

# Agent guide

Rules for any coding agent working in this repository.

## Commands

- Build: `cargo build`
- Test: `cargo test`
- Format check: `cargo fmt` with the check option
- Lint: `cargo clippy` on all targets, with warnings denied

The exact invocations are in [ci.yml](.github/workflows/ci.yml). Run them before you commit.

## Rules

- No `unwrap` or `expect` outside tests.
- Unsafe code is forbidden.
- Text that comes from GitHub goes through the sanitizer before it reaches the screen. Once the sanitizer exists, it is the only route.
- Tests use the std server in `tests/support`. They never call the real GitHub.
- Pin dependencies with an exact `=` version.
- Put an SPDX header on every file.
- Write a failing test first.
- Never run a hard reset with git (the reset command in hard mode).
- Sign and sign off every commit: `git commit -S -s`. Never push to the default branch.
- Read the [contributing guide](CONTRIBUTING.md) for commit and pull request rules.
