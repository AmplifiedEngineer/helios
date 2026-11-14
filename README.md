<!-- SPDX-License-Identifier: Apache-2.0 -->

# helios

helios is planned as a terminal UI. It will list your own pull requests and the pull requests that need you. It will work across GitHub accounts and hosts. In release 1 it will be read-only: it will never write to GitHub. See Status for what works today.

## Status

Release 1 is in progress. It is not released yet. The terminal UI works: it lists your pull requests and the ones that need you from every `gh` account, opens a pull request with its tabs, files and diffs, and refreshes every 120 seconds. `--smoke` queries your accounts and prints the counts. helios never writes to GitHub.

## Requirements

- The GitHub CLI, `gh`, installed and logged in.
- helios will take its accounts from `gh`. It will have no login of its own.

## Usage

```
helios [options]
```

### Flags

| Flag | Meaning |
|------|---------|
| `--hosts <hosts>` | Comma-separated hosts to include. `--smoke` honours it. Env: `HELIOS_HOSTS`. |
| `--refresh-secs <secs>` | Refresh interval, 30 to 3600 seconds. The value is checked; nothing uses it yet. Env: `HELIOS_REFRESH_SECS`. |
| `--theme <name>` | Start theme: `default`, `spectrum`, `dark`, `light` or `mono`. Beats the `[theme]` section of the config file. Env: `HELIOS_THEME`. |
| `--drafts` | Count draft pull requests. `--smoke` honours it. |
| `--smoke` | Query every account once, print the Mine and Needs me counts, and exit. It does not start the UI. Exit code 1 when there are no accounts or no host answers. |
| `--version`, `-V` | Print the version and exit. |
| `--help` | Print the usage and exit. |

A flag beats its environment variable, and the environment beats the config file. `HELIOS_CONFIG` names the config file; the default is `$XDG_CONFIG_HOME/helios/config.toml`, else `~/.config/helios/config.toml`. A missing file means the defaults. A bad file stops startup with a message that names the file, the line and the key. Helios only reads the file. `--help`, `--version` and `--smoke` do not read it. `HELIOS_GH` names the `gh` program to run; the default is `gh`.

### Keys

The footer shows the keys that work on the screen you are on. `?` opens the same list.

| Key | Action |
|-----|--------|
| `j/k` | move, or scroll a diff or a long tab. The arrow keys work too. |
| `g/G` | first or last |
| `Enter/l` | open the pull request |
| `Enter/Space` | open or close the file under the cursor on the Files tab |
| `e/E` | open or close every file on the Files tab |
| `x` | open or close the tests group on the Files tab |
| `v` | open the full-screen diff of the file under the cursor |
| `Esc/h` | back. `Backspace` works too. |
| `m` | switch between Mine and Needs me |
| `r` | refresh |
| `f` | filter Needs me: all, review or assigned |
| `d` | toggle drafts |
| `Tab` | next tab |
| `Shift-Tab` | previous tab |
| `t` | switch the color theme: default, spectrum, dark, light or mono |
| `o` | open the pull request in the browser |
| `?` | help |
| `q` | quit |
| `Ctrl-C` | quit |

## Release

A release is a version tag. The release workflow in [release.yml](.github/workflows/release.yml) does the rest.

1. Open a pull request that sets `version` in `Cargo.toml` to the new version, such as `0.2.0`, and update `Cargo.lock`. Merge it.
2. Pull the merge commit on main, then create the tag `v0.2.0` on it, with a `v` before the version, and push the tag.
3. The workflow stops if the tag and the `Cargo.toml` version differ. Otherwise it opens a draft release and builds `helios` for macOS on Apple silicon and Intel, and for Linux on x86_64 and arm64 as static musl binaries.
4. Each target gives `helios-<version>-<target>.tar.gz`, which holds the single `helios` binary. The workflow attests every archive, checks the archives against their attestations, then writes and attests `SHA256SUMS` and publishes the release as the latest.
5. The Homebrew tap, `amplifiedengineer/homebrew-tap`, checks for new Helios releases every 30 minutes and opens its own bump pull request. Nothing here needs to notify it.

A published release is never built again, and a tag never moves. If a build fails before publish, fix the cause and run the failed jobs again from the Actions tab. The release stays a draft until every archive passes the checks. Check a download with `gh attestation verify`, which takes the archive and the repository `amplifiedengineer/helios`.

## Known gaps

These are planned limits of release 1.

- A review request made to a team will not count. Only requests made to you will.
- A pull request will leave Needs me once you review it. GitHub drops the request when a review is submitted.
- Search will stop at 1000 results. helios will then mark the list as partial.

## License

Apache-2.0. See [LICENSE](LICENSE).
