use crate::package_config::{ensure_package_config_default, game_package_config};

const PACKAGE: &str = "example-rust";
const DEFAULT_GREETING: &str = "Welcome from example-rust!";

crate::game_hook!(on_login, fn example_rust_on_login(ctx, payload) {
    ensure_package_config_default(ctx, PACKAGE, "greeting", DEFAULT_GREETING);
    let configured = ctx.db.game_package_config().by_package_key()
        .filter((PACKAGE, "greeting")).next().map(|row| row.value);
    let message = greeting(configured.as_deref()).to_string();
    if let Err(reason) = crate::actor::system_message(ctx, payload.character_guid, message) {
        spacetimedb::log::warn!("{PACKAGE}: welcome failed: {reason}");
    }
});

fn greeting(configured: Option<&str>) -> &str {
    configured
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(DEFAULT_GREETING)
}

#[cfg(test)]
mod tests {
    #[test]
    fn blank_config_keeps_the_default_greeting() {
        assert_eq!(super::greeting(Some("  ")), "Welcome from example-rust!");
    }
}
