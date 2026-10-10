# Rust asks a Runtime Script

On login, Rust calls `ask()` for Package Event `example-all.welcome`. `scripts/welcome.lua`
answers `1`, so Rust sends a welcome through `actor::system_message`. Change the Script Answer
to `0` to suppress it.

When no script answers, Rust reads Package Config key `welcome`. `false` suppresses the greeting;
`true`, a missing value, or an invalid value sends it. The default is `true`. Package tests run
the authored Lua in the Runtime Script Host and check the fallback.

Install with `./lyracore packages add example-all`, then publish through the normal realm
update. Apply its Script Artifact with `./lyracore packages replay` on your development topology.
After editing the Runtime Script, run `./lyracore packages build` and commit its Script
Artifact and Build Identity together. Its Script ID is 100302. Give copies distinct IDs before
installing them together.
