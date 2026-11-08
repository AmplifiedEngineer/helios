<!-- SPDX-License-Identifier: Apache-2.0 -->

# forgebird

forgebird is planned as a terminal UI. It will list your own pull requests and the pull requests that need you. It will work across GitHub accounts and hosts. In release 1 it will be read-only: it will never write to GitHub. None of this is built yet.

## Status

Release 1 is in progress. It is not released yet. The program does nothing yet. Only library code exists: listing `gh` accounts, running `gh` safely and cleaning text from GitHub.

## Requirements

- The GitHub CLI, `gh`, installed and logged in.
- forgebird will take its accounts from `gh`. It will have no login of its own.

## Usage

Flags and keys are documented once the CLI lands.

## Known gaps

These are planned limits of release 1.

- A review request made to a team will not count. Only requests made to you will.
- A pull request will leave Needs me once you review it. GitHub drops the request when a review is submitted.
- Search will stop at 1000 results. forgebird will then mark the list as partial.

## License

Apache-2.0. See [LICENSE](LICENSE).
