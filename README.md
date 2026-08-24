# LyraCore Packages

This repository is LyraCore's Official Package Collection. Each visible top-level directory is one
independently installable Package. There is no separate registry or index file.

`main` tracks LyraCore `main`. When LyraCore starts publishing releases, this repository will use
matching tags for compatible Package revisions.

## Packages

- [`example`](example/) demonstrates the smallest compiled Package and a notify-only gameplay hook.

Packages compile into LyraCore's Module and run as trusted code. Review a Package as you would a
core patch before installing it.

## Compatibility

CI installs every Package into a clean checkout of
[`LyraCoreProject/LyraCore`](https://github.com/LyraCoreProject/LyraCore) and runs the Module library
tests against core tip. A Package API or schema incompatibility fails the collection build.

To run the same check locally against a clean LyraCore checkout:

```bash
./.github/check-core-tip.sh /path/to/LyraCore
```

The check refuses a core checkout whose `packages/` directory is not empty.

## License

Package source in this repository is available under the [MIT License](LICENSE).
