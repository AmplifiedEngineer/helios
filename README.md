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
| `--print-config` | Print the effective settings as TOML and exit. It reads the config file and applies `--theme` and `HELIOS_THEME`. It starts no UI and makes no network call. |
| `--version`, `-V` | Print the version and exit. |
| `--help` | Print the usage and exit. |

A flag beats its environment variable, and the environment beats the config file. `HELIOS_CONFIG` names the config file. `HELIOS_GH` names the `gh` program to run. The default is `gh`. See Config file for the file.

### Keys

The footer shows the keys that work on the screen you are on. `?` opens the same list. The footer and the list show your own keys when you rebind them. See Config file.

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

`q`, `Ctrl-C`, `?` and `Esc` keep their meaning. You cannot rebind them.

### Themes

helios ships five themes: `default`, `spectrum`, `dark`, `light` and `mono`. `mono` draws no color. Press `t` to switch to the next theme while helios runs. Pick the start theme with `--theme`, with `HELIOS_THEME` or with `name` in the `[theme]` section of the config file. The flag beats the environment, and the environment beats the file. When `NO_COLOR` is set, helios draws with `mono` whatever theme you pick.

## Config file

The config file is optional. helios reads it at startup and never writes it. A change takes effect the next time you start helios.

The file lives in one of these places. The first one that applies wins.

1. The path in `HELIOS_CONFIG`.
2. `$XDG_CONFIG_HOME/helios/config.toml`.
3. `~/.config/helios/config.toml`.

A missing file means the defaults. A bad file stops startup with a message that names the file, the line and the key. `--help`, `--version` and `--smoke` do not read the file. `--print-config` reads it.

Every setting is optional. The file has five sections. Here is one example of each.

```toml
[layout]
list_percent = 40
narrow_below = 100
preview_delay_ms = 300

[columns]
mine = ["repo", "number", "title", "checks", "updated"]
needs_me = ["author", "title", "review", "updated"]

[keys.files]
toggle_tests = ["z"]

[theme]
name = "dark"

[files]
test_dirs = ["tests", "e2e"]
test_files = ["*_test.go", "*.spec.ts"]
collapsed = true
```

- `[layout]` sets the width of the list pane as a percent (20 to 70), the width below which helios shows one pane at a time (40 to 400), and the wait before the preview loads in milliseconds (0 to 5000).
- `[columns]` sets which columns each list shows and in what order. The names are `host`, `repo`, `number`, `title`, `author`, `tag`, `draft`, `review`, `checks` and `updated`.
- `[keys]` rebinds keys. See Rebinding keys.
- `[theme]` sets the start theme.
- `[files]` sets which paths count as tests, and whether the Files tab starts with every file collapsed.

### Rebinding keys

Each scope has its own table: `[keys.list]`, `[keys.detail]`, `[keys.files]` and `[keys.diff]`. Each entry names an action and lists its keys. A key is one character, `Ctrl-` and one character, or one of `Enter`, `Space`, `Tab`, `Shift-Tab`, `Up`, `Down`, `Esc`, `Backspace`, `Home` and `End`.

An entry replaces the default keys of that action in that scope. Other actions and other scopes keep their keys. One key cannot run two actions in the same scope.

| Scope | Actions |
|-------|---------|
| `list` | `move_down`, `move_up`, `first`, `last`, `open`, `toggle_view`, `refresh`, `filter`, `toggle_drafts`, `theme`, `open_in_browser`, `help`, `quit` |
| `detail` | `scroll_down`, `scroll_up`, `first`, `last`, `back`, `next_tab`, `previous_tab`, `theme`, `open_in_browser`, `help`, `quit` |
| `files` | `move_down`, `move_up`, `first`, `last`, `toggle_file`, `open_all`, `close_all`, `toggle_tests`, `full_diff`, `back`, `next_tab`, `previous_tab`, `theme`, `open_in_browser`, `help`, `quit` |
| `diff` | `scroll_down`, `scroll_up`, `first`, `last`, `back`, `theme`, `open_in_browser`, `help`, `quit` |

Run `helios --print-config` to see every action with its keys. Lines that start with `#` show the defaults.

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
