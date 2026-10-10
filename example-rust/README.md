# Welcome from Rust

One `on_login` hook sends a System Message through `actor::system_message`. It seeds Package
Config key `greeting` on first login and reads it on each login. A blank value uses the default
welcome. The Package test covers that fallback.

Install with `./lyracore packages add example-rust`, then run `./lyracore packages apply` on your
development topology. It publishes the Module and repairs schedules. On your development topology, `./lyracore packages config example-rust greeting "Hello!"`
changes the next welcome without another publish.
