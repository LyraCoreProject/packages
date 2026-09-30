# Playerbots tests

This crate owns the Package's behavior, migration, capacity and companion acceptance tests.
Core provides `lyracore-test-support` for private Standalone processes, Module builds and Gateway
builds. The test runner resolves those dependencies from the selected Core checkout.

Run from the Collection checkout:

```sh
./.github/check-playerbots.sh /path/to/LyraCore test --locked --no-run
./.github/check-playerbots.sh /path/to/LyraCore test --locked \
  --test playerbots_quest_catalog playerbots_missing_quest_target_retry_ \
  -- --ignored --test-threads=1
```

Historical migration tests need the preceding artifacts and source manifests prepared by
`.github/workflows/core-tip.yml`. Companion wire tests also need the pinned wire client. The
imported-world cases require Operator-supplied geometry. Missing inputs fail their checks.

`gateway_coordinator.rs` exercises the Gateway's private coordinator seam. Core admits that source
only when `LYRACORE_COORDINATOR_TEST_SOURCE` is set for a test build:

```sh
./.github/check-playerbots.sh /path/to/LyraCore coordinator \
  stdb::subscriptions::package_tests -- --include-ignored --test-threads=1
```

The runner keeps build artifacts outside this Package and uses Core's pinned Rust toolchain.
Server evidence does not complete attended acceptance with an unmodified 1.12.1.5875 client.
