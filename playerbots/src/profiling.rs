use spacetimedb::{log_stopwatch::LogStopwatch, ReducerContext};

/// Optional phase timings. Each span carries the same identity as the decision timing.
pub(super) struct Decision {
    identity: Option<String>,
    current: Option<LogStopwatch>,
}

impl Decision {
    pub(super) fn start(ctx: &ReducerContext, guid: u64, generation: u64, now: i64) -> Self {
        let identity = super::config_parsed(ctx, "decision_profile", false).then(|| {
            format!("playerbots_phase guid={guid} generation={generation} observed_micros={now}")
        });
        let mut profile = Self {
            identity,
            current: None,
        };
        profile.phase("party");
        profile
    }

    pub(super) fn phase(&mut self, name: &str) {
        drop(self.current.take());
        self.current = self
            .identity
            .as_ref()
            .map(|identity| LogStopwatch::new(&format!("{identity} phase={name}")));
    }
}

pub(super) fn movement(ctx: &ReducerContext, guid: u64) -> Option<LogStopwatch> {
    super::config_parsed(ctx, "decision_profile", false).then(|| {
        LogStopwatch::new(&format!(
            "playerbots_route guid={guid} observed_micros={}",
            ctx.timestamp.to_micros_since_unix_epoch()
        ))
    })
}
