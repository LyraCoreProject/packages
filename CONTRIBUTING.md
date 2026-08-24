# Contributing

Each visible top-level directory is one Package. Keep unrelated Packages independent and do not add
a separate collection manifest.

Before opening a pull request:

1. Read LyraCore's `CODING_STANDARDS.md` and `CONTEXT.md` at the core revision you target.
2. Run `./.github/check-core-tip.sh /path/to/LyraCore` against a clean LyraCore checkout.
3. Confirm the Package contains no Blizzard client files, credentials, or private source.

Pull requests must pass the `module` core-tip compatibility check before merge. Maintainers may use
their GitHub branch-protection bypass only to recover the repository or repair CI.
