# Wave defense tests

This crate runs Package checks against an explicit Core checkout. Durable tests publish the
selected Module into private `Standalone` processes. They do not use an existing Realm.

The command and round tests need the SpacetimeDB 2.7.1 CLI, Core's pinned Rust toolchain and Wasm
target, and an Operator-owned WoW 1.12.1 client. Set `LYRACORE_TEST_CLIENT_DATA` to that client's
`Data` directory. The tests import its actual spell and talent catalogues before staging reserved
fixture creatures. They never invent spells at Blizzard identifiers. The importer reads a temporary
private CLI configuration, which the test deletes after the import. `LYRACORE_TEST_IMPORTER` can
select a prebuilt importer; otherwise the tests build the selected Core's importer.

Run from the Package Collection:

```sh
LYRACORE_TEST_CLIENT_DATA='/path/to/client/Data' \
  ./.github/check-wave-defense.sh /path/to/LyraCore test --locked \
  --test commands --test rounds -- --ignored --test-threads=1
```

On macOS, set `CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS` to
`-C link-arg=-Wl,-undefined,dynamic_lookup` for native test executables.

The debug-only fixture stages ten human Characters on separate Accounts in two balanced rosters.
The tests claim their Accounts and send commands through `gw_client_command` with World Session
Tokens. Queue consent, premade preservation, separate Instances, replay, stale choices, preparation,
shared levels, progress and reset are checked through durable rows and addon replies.

Round tests accelerate mode deadlines. Wave clearing spawns creatures through the production round
code and invokes the real creature death kernel. Base-loss checks place live enemies at the base and
invoke production base damage. These checks prove state transitions with debug fixtures. They do not
prove a 30-minute run, map routes, real combat balance or client rendering.

Run addon checks from `wave-defense` with Lua 5.0:

```sh
lua testsuite/addon/protocol.lua
lua testsuite/addon/ui.lua
```

The addon UI test supplies an explicit WoW API shim and checks UI logic at a narrow width. It does
not run Benilla. An attended Benilla run must still check routes, targeting, shops, addon transport
and five-person combat.
