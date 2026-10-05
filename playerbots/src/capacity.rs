//! A host renews this short lease only while its disk reserve is available.

use spacetimedb::ReducerContext;

pub(super) fn require_capacity(ctx: &ReducerContext) -> Result<(), String> {
    let lease = super::config_value(ctx, "capacity_until_micros");
    if permits(lease.as_deref(), ctx.timestamp.to_micros_since_unix_epoch()) {
        Ok(())
    } else {
        Err("playerbots suspended: host disk capacity lease is invalid or expired".to_string())
    }
}

fn permits(lease: Option<&str>, now: i64) -> bool {
    match lease {
        // Unmanaged Realms retain their existing behavior. Once configured, a stale or malformed
        // lease refuses work even if the host monitor has stopped running.
        None => true,
        Some(value) => value.parse::<i64>().is_ok_and(|until| until > now),
    }
}

#[cfg(test)]
mod tests {
    use super::permits;

    #[test]
    fn unmanaged_realms_do_not_require_a_host_monitor() {
        assert!(permits(None, 100));
    }

    #[test]
    fn managed_work_requires_an_unexpired_capacity_lease() {
        assert!(permits(Some("101"), 100));
        for value in ["100", "99", "0", "-1", "", "broken", "9223372036854775808"] {
            assert!(!permits(Some(value), 100), "{value}");
        }
    }
}
