<!-- SPDX-License-Identifier: Apache-2.0 -->

# Contributing to forgebird

Thanks for helping. Keep changes small and clear.

## Workflow

- Open small pull requests. One idea per pull request.
- Write a failing test first. Then write the code that makes it pass.
- Squash merge is the only merge method.

## Commits

- Use [conventional commits](https://www.conventionalcommits.org/), such as `feat(ui): add a filter`.
- Sign every commit and sign it off: `git commit -S -s`.
- The dco check fails a commit with no `Signed-off-by` line.

## Code rules

- Pin every dependency with an exact `=` version in `Cargo.toml`.
- Put an SPDX header on every file: `SPDX-License-Identifier: Apache-2.0`.
- Unsafe code is forbidden.
- No `unwrap` or `expect` outside tests.

## CI checks

A pull request must pass these checks. The exact commands are in [ci.yml](.github/workflows/ci.yml).

- lint: format, clippy and shellcheck.
- test: the test suite on each operating system.
- dco: the sign-off check.
- zizmor: the workflow security lint.

## Security

Report a vulnerability as [SECURITY.md](SECURITY.md) describes. Do not use a public issue.
