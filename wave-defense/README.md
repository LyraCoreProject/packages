# Wave Defense

Five Characters defend an instanced Scarlet Monastery courtyard through 15 waves and three bosses.
The Package adds matchmaking, round progression, shops and an ordinary 1.12 addon. It requires the
companion Core changes and the Playerbots Package from the same source revision. Stock Benilla
needs no binary changes.

The prototype is deployed on Argus at `159.69.88.70`. Automated round tests advance deadlines and
resolve creature deaths; they do not establish combat balance or a 30-minute completion time.
An attended five-person run remains necessary to assess the play experience.

## Play

Copy [`client/addons/WaveDefense`](client/addons/WaveDefense/) into `benilla-config/AddOns/` beside
Benilla. Enable the addon, enter the dedicated realm and use `/wd` to open the lobby controls.
Select the specialization for your Character's class, then join the queue. To change class, return
to ordinary character selection. Every human in a premade must join the queue.

Teams have one tank, one healer and three damage roles. The queue preserves premades and fills
missing roles with labeled bots after 60 seconds. Each team gets its own Instance. The shared lobby
holds up to 200 Characters, with at most 64 active matches; these are bounds, not measured capacity.

The objective dock sits at the top of the screen. Reward choices appear in the center during
preparation. Choose once at level 1 and every five levels through 60. You can bank choices; combat
always banks them until preparation. The reward pool uses the chosen class and talent tree and
excludes imported spells with no implemented effect. Learned active spells gain ranks with team
level. Role spells and ordinary class equipment proficiencies are guaranteed.
The first draft pool omits unsupported Core effects, including traps, totems, Feign Death and
Innervate. Bear combat currently uses Core's weapon damage rules; passive mana regeneration while
shifted is not implemented. These are prototype limits, not complete Classic class fidelity.

Preparation lasts 30 seconds before the first wave and 20 seconds thereafter. All-ready starts the
next wave early. Each clear refills health and mana and pays equal personal gold. Visit the
Quartermaster for level-appropriate gear and consumables. Match combat consumes no ammunition,
reagents or durability. Health, mana and other ordinary combat resources still matter.

The base starts at 10,000 health. Boss clears restore 2,500. Boss waves use Cleave, War Stomp, then
both; the team must clear every enemy to advance. A destroyed base ends the round. Death starts a
15-second respawn timer. Resurrection spells can work in combat and share a 120-second cooldown.

After 15 seconds offline, a separate bot takes your role. Logging back into the same Character
restores your place and choices. Leaving explicitly makes the replacement permanent. Matches with
no humans present close after two minutes. Returning to the lobby removes match levels, money,
items, learned spells and talents. Account wave, boss and win totals remain; they grant no stats.

## Prepare a dedicated realm

Use a separate single-Shard realm that hosts Realm-core and the Instance Pool. Enabling this mode
resets its Characters to the level-1 lobby; do not enable it on an ordinary progression realm.

1. Install both `playerbots` and `wave-defense` into the matching Core checkout. Build Core and
   regenerate its Gateway bindings with `./lyracore bindings`.
2. Follow [content-notes.md](content-notes.md) to map local ClassicDB and 1.12 client data, generate
   the authored creature and item Deltas, and verify courtyard geometry. Generated game content
   remains local and is not distributed with the addon.
3. Import the normal Character creation data, spells, talent trees, spell chains, items and creature
   families. Starter equipment requires the ordinary item catalogue as well as authored shop gear.
   Apply the authored Deltas, import the `scarlet-courtyard` geometry and prepare its Navigation
   Coverage. The importer profile adds no ordinary dungeon population to match Instances.
4. Start the matching Gateway against the single Shard. Its renewable certificate allows local
   Party formation. As Operator, invoke `wave_defense_configure true` only after content import.
   Configuration checks content, floors, routes, sight lines and reserved vendor identifiers.
5. Log in again to enter the lobby. Reconfigure after an item-family import, which replaces vendor
   stock. To disable, wait for every match to return to the lobby, then invoke
   `wave_defense_configure false`.

The mode does not supply account registration, public hosting, TLS termination or an update service.
Those belong to the realm's deployment. Addon requests carry intent; authenticated Module rules
own queue admission, rewards, money, progression and match outcomes.

## Verify locally

The integration suite starts private SpacetimeDB Standalone processes on random loopback ports.
It does not use an existing realm. Set `LYRACORE_TEST_CLIENT_DATA` to your 1.12 `Data` directory.
The courtyard test also requires `LYRACORE_TEST_WORLD_DUMP` and `WAVE_DEFENSE_BASE_CONTENT`, as
[content-notes.md](content-notes.md) describes.

From the collection, with a clean Core checkout:

```bash
.github/check-wave-defense.sh /path/to/LyraCore test -- --ignored --test-threads=1
```

The wrapper installs the collection for the check and restores the checkout afterward. It refuses
pre-existing Package symlinks. On macOS, native Module integration tests require
`CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS='-C link-arg=-Wl,-undefined,dynamic_lookup'`.
Use Bash 4 or newer for the collection wrapper.

The addon tests use Lua 5.0:

```bash
cd wave-defense
lua testsuite/addon/protocol.lua
lua testsuite/addon/ui.lua
```

The actual stock Benilla addon harness also loads WaveDefense with zero load or session errors.
Injected preparation and reward replies show the top dock and centered choices, and `/wd` banks the
choice panel. This checks the real interface VM with fixture messages, not a live network session.
Core's `docs/wave-defense.md` and `docs/wave-defense-protocol.md` describe the rules and wire format.

The collection check passes all 1,730 Core library tests and 37 Package API checks. Private durable
tests also pass for the full round, queue eligibility, progression, combat, pets, forms and reconnect.
Formatting and comment review are complete.

The complete Linux deployment gate passes on Argus, including all Gateway targets and 146 schema
parity checks. The earlier macOS run had socket errors and a hanging login-cancellation test;
those failures did not recur on Linux.

The deployed realm passes authenticated login, level-one lobby entry, the normal sixty-second
bot-fill queue, a five-member Party, the initial draft choice and lobby return through a Headless
Client. Public login also passes after cutover. Install the addon separately on stock Benilla and
use the existing Discord signup bot to create a fresh account. The previous realm's accounts and
Characters remain in its retained databases.
