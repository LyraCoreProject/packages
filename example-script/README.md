# Welcome with Runtime Scripts

`welcome.ts` sends one chat line on login. `ding.lua` sends one on level-up. Both use
`send_chat` on the Character carried by the event. This Package needs no Rust.

Install with `./lyracore packages add example-script`, then apply the committed Script Artifact
with `./lyracore packages apply` on your development topology. It builds missing or stale artifacts when needed. After editing
either source, run
`./lyracore packages build` and commit the Script Artifact and its Build Identity together.

The Script IDs are 100300 and 100301. Give copies distinct IDs before installing them together.
