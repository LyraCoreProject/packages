# Rust asks a Runtime Script

On login, Rust calls `ask()` for Package Event `example-all.welcome`. `scripts/welcome.lua`
registers its `welcome(event)` function with `events.package.on("welcome", welcome)` and answers
`1`, so Rust sends a welcome through `actor::system_message`. Change the Script Answer to `0` to
suppress it.

When no script answers, Rust reads Package Config key `welcome`. `false` suppresses the greeting;
`true`, a missing value, or an invalid value sends it. The default is `true`. Package tests run
the compiled Script Artifact Lua in the Runtime Script Host and check the fallback.

Install with `./lyracore packages add example-all`, then run `./lyracore packages apply` on your
development topology. It builds artifacts when needed, publishes the Module, repairs schedules,
and applies the Script Artifact.
After editing the Runtime Script, run `./lyracore packages build`. Commit the sources,
`script-ids.json`, Script Artifact and its Build Identity together. The Runtime Script Toolchain
manages IDs in `script-ids.json`; keep its entries when you remove a source. Use
`./lyracore packages new NAME --from example-all` to make a copy with new IDs.
