# Welcome with Runtime Scripts

`welcome.ts` registers `welcome` with `events.player.onLogin`. `ding.lua` registers `ding` with
`events.player.onLevelUp`. Both send one chat line to `event.player`, the Character the typed event
guarantees. This Package needs no Rust.

Install with `./lyracore packages add example-script`, then apply the committed Script Artifact
with `./lyracore packages apply` on your development topology. It builds missing or stale artifacts
when needed. After editing either source, run `./lyracore packages build`. Commit the sources,
`script-ids.json`, Script Artifact and its Build Identity together.

The Runtime Script Toolchain manages IDs in `script-ids.json`. Keep the ledger, including entries
for removed sources. Each Runtime Script keeps the name `<package>.<file stem>` and its ID when
you rename its handler. Use `./lyracore packages new NAME` to make a copy with new IDs.
