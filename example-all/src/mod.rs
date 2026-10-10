use crate::package_config::{ensure_package_config_default, game_package_config};

const PACKAGE: &str = "example-all";
const WELCOME_EVENT: &str = "example-all.welcome";

crate::game_hook!(on_login, fn example_all_on_login(ctx, payload) {
    ensure_package_config_default(ctx, PACKAGE, "welcome", "true");
    let fallback = ctx.db.game_package_config().by_package_key()
        .filter((PACKAGE, "welcome")).next().map(|row| row.value);
    let answer = crate::script_binding::ask(ctx, WELCOME_EVENT, payload.character_guid, 0);
    if should_welcome(answer, fallback.as_deref()) {
        if let Err(reason) = crate::actor::system_message(
            ctx, payload.character_guid, "Welcome from example-all!".to_string(),
        ) {
            spacetimedb::log::warn!("{PACKAGE}: welcome failed: {reason}");
        }
    }
});

fn should_welcome(answer: Option<f64>, fallback: Option<&str>) -> bool {
    match answer {
        Some(number) => number > 0.0,
        None => fallback
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(true),
    }
}

#[cfg(test)]
mod tests;
