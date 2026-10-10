# LyraCore Packages

This repository is LyraCore's Official Package Collection. Each visible top-level directory is one
independently installable Package. There is no separate registry or index file.

Each Package API version has a tag, such as `api-v1`. After the compatibility checks pass on
`main`, CI moves the tag for the checked Core revision's Package API version to that collection
commit. Tags for older API versions stay at their last compatible commit.

`lyracore packages add <name>` and `lyracore packages update` select the tag from the version in
the checkout's `docs/package-api.md`. A missing tag refuses the operation. Each installed Package's
Provenance Stamp records the exact collection commit it came from.

For a running development Realm:

```bash
./lyracore packages add example-script
./lyracore packages apply
```

`apply` builds missing or stale artifacts, publishes the Module when Rust Packages require it,
and applies artifacts to the recorded development topology. Pass Shard names to select targets.
Use `--check` to prepare and inspect without Realm changes. Client content uses `client sync`.
Update Core to a revision whose CLI provides `packages apply` before following these instructions.

## Packages

Start with these Reference Packages. Each adds a small welcome at one level of the Package API.

| Package | What to read |
| --- | --- |
| [`example-script`](example-script/) | TypeScript on login and Lua on level-up, with one chat line each |
| [`example-client`](example-client/) | An addon and a UI Transform |
| [`example-data`](example-data/) | A Datascript that clones one spell, with a Delta built locally |
| [`example-rust`](example-rust/) | A Rust hook, Package Config and a Package test |
| [`example-all`](example-all/) | Rust asks a Runtime Script, with Package Config as the fallback |

- [`dungeons`](dungeons/) holds scripted dungeon choreography, one submodule per dungeon.
  Deadmines is its first dungeon.
- [`playerbots`](playerbots/) fields a population of session-less Characters a player can group
  with, so a small realm still has a party to test content with.

Packages compile into LyraCore's Module and run as trusted code. Review a Package as you would a
core patch before installing it.

## Compatibility

CI installs every Package into a clean checkout of
[`LyraCoreProject/LyraCore`](https://github.com/LyraCoreProject/LyraCore) and runs the Module library
tests against the Core revision pinned in `.github/workflows/core-tip.yml`. A Package API or
schema incompatibility fails the collection build.

To run the same check locally against a clean LyraCore checkout:

```bash
./.github/check-core-tip.sh /path/to/LyraCore
```

The check temporarily links collection Packages into Core. It restores any in-tree Packages with
matching names when it exits.

## Contributing

Changes land through pull requests after the core-tip compatibility check passes. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Package source in this repository is available under the [MIT License](LICENSE).
