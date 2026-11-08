<!-- SPDX-License-Identifier: Apache-2.0 -->

# Security Policy

## Reporting a vulnerability

Do not open a public GitHub issue for a security problem.

Use GitHub private vulnerability reporting: [report a vulnerability](https://github.com/amplifiedengineer/helios/security/advisories/new).

Please include:

- What the problem is.
- Steps to reproduce it.
- Which version or commit is affected.
- What an attacker can do with it.
- A suggested fix, if you have one.

## Supported versions

Release 1 is not out yet. Fixes go to the main branch. After release 1, fixes target the latest release only.

## Scope

In scope: flaws in this repository's code. Examples are code execution, injection, unsafe handling of text from GitHub, and leaking a token.

Out of scope:

- Flaws in third-party dependencies. Report those upstream.
- Flaws in `gh` or in GitHub itself.
- Issues that need write access to this repository.
- Issues that need physical access to your machine.
