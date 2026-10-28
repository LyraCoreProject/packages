use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{poll_until, Standalone, TableSubscription, POLL_TIMEOUT};

pub struct LiveEvents {
    subscription: TableSubscription,
    table: &'static str,
    marker: u64,
}

impl LiveEvents {
    pub fn subscribe(node: &Standalone, table: &'static str, filter: &str) -> Self {
        static NEXT_MARKER: AtomicU64 = AtomicU64::new(5_099_999);
        let marker = NEXT_MARKER.fetch_add(1, Ordering::Relaxed);
        node.assert_sql(&format!(
            "INSERT INTO game_package_config (id, package_name, key, value) \
             VALUES ({marker}, 'fixture.subscription', 'transaction', 'initial')"
        ));
        let subscription = node.subscribe(&[
            &format!("SELECT * FROM {table} WHERE {filter}"),
            &format!("SELECT * FROM game_package_config WHERE id = {marker}"),
        ]);
        Self {
            subscription,
            table,
            marker,
        }
    }

    pub fn rows(&self, node: &Standalone) -> Vec<BTreeMap<String, String>> {
        static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(1);
        let transaction = NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed).to_string();
        node.assert_sql(&format!(
            "UPDATE game_package_config SET value = '{transaction}' WHERE id = {}",
            self.marker
        ));
        // A marker on the same stream fences all earlier committed events.
        assert!(
            poll_until(POLL_TIMEOUT, || {
                self.subscription
                    .inserts("game_package_config")
                    .iter()
                    .any(|row| row["id"] == self.marker && row["value"] == transaction)
            }),
            "the event subscription did not receive transaction {transaction}"
        );
        self.subscription
            .inserts(self.table)
            .into_iter()
            .map(|row| {
                row.as_object()
                    .expect("event row is an object")
                    .iter()
                    .map(|(column, value)| {
                        let value = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        (column.clone(), value)
                    })
                    .collect()
            })
            .collect()
    }
}
