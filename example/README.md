# Example

`example` is the smallest compiled LyraCore Package. It registers an `on_login` hook and sends the
logging-in Character one private System Message using the guid from the typed event payload.

The Package defines no table or reducer. It ships no client file, persistent data, or Blizzard
asset. Its purpose is to prove the Package folder contract and provide a clean starting point for
`lyracore packages new`. Removing it from core's `packages/` folder removes both the hook and the
message without a core edit.

To try it before the Package CLI ships, copy this directory to `packages/example` in a LyraCore
checkout and run:

```bash
cargo test -p lyracore-module --lib
```
