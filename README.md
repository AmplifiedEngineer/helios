<!-- SPDX-License-Identifier: Apache-2.0 -->

# forgebird

forgebird is a terminal UI. It lists your own pull requests and the pull requests that need you. It works across GitHub accounts and hosts. In release 1 it is read-only: it never writes to GitHub.

## Status

Release 1 is in progress. It is not released yet.

## Requirements

- The GitHub CLI, `gh`, installed and logged in.
- forgebird takes its accounts from `gh`. It has no login of its own.

## Usage

Flags and keys are documented once the CLI lands.

## Known gaps

- A review request made to a team does not count. Only requests made to you do.
- A pull request leaves Needs me once you review it. GitHub drops the request when a review is submitted.
- Search stops at 1000 results. forgebird then marks the list as partial.

## License

Apache-2.0. See [LICENSE](LICENSE).
