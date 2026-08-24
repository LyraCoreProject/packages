# Example

`example` is the smallest compiled LyraCore Package. It registers a notify-only login hook and reads
the typed event payload without changing game state.

The Package has no table, reducer, client file, or persistent data. Its purpose is to prove the
Package folder contract and provide a clean starting point for `lyracore packages new`.

To try it before the Package CLI ships, copy this directory to `packages/example` in a LyraCore
checkout and run:

```bash
cargo test -p lyracore-module --lib
```
