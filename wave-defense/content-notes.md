# Authored content and courtyard verification

Wave Defense ships authoring code. Generated creature and item rows come from the Operator's own
local content and stay outside source control. The four templates use Package Creature Range entries
15189001 through 15189004. The 51 shop items use Package Item Range entries 7189000 through 7189050.
Vendor stock uses fixed row identifiers 15189003000 through 15189003050 and belongs only to NPC
15189003. Configuration refuses a collision with another vendor.

## Source inputs

The source inspection used [CMaNGOS ClassicDB commit ec4f596](https://github.com/cmangos/classic-db/tree/ec4f596146be6467ea93c57397858e329e2db852),
with `Full_DB/ClassicDB_1_12_1_z2815.sql.gz`. Its SHA-256 is
`4f92db520868ab4e566726f68b5b2e380ae781209beaf22237b4f7f04600d0c0`.
The authoring code references these source templates:

| Source entry | Source template | Authored use |
| --- | --- | --- |
| 1525 | Rotting Dead | Courtyard Invader |
| 1530 | Rotting Ancestor | Courtyard Warlord |
| 68 | Stormwind City Guard | Defense Quartermaster |
| 299 | Young Wolf, beast family 1 | Defense Wolf |

The map is `MonasteryInstances`, map 189. The base at `947.376, 1375.67, 18.1066` follows Cathedral
courtyard spawn 1890179. The center front at `909.251, 1378.57, 18.0794` follows spawn 1890191.
The other fronts offset the center by six yards in each y direction. The quartermaster is six yards
from the base in the x direction. Configuration checks their floors, routes and sight lines against
the imported geometry.

A local read found the client archives at
`/Users/mathiashartveit/Documents/Private/wowserver/client/RetroWoW 1.12.1/Data`.
The extraction used its `dbc.MPQ`, terrain and model archives, and patch chain. No extracted client
files enter this Package.

## Build the authored Deltas

Run these commands from Core. `CLIENT_DATA` names an existing client `Data` directory. `WORLD_DUMP`
names the locally downloaded SQL dump. These commands map data without contacting a Realm.

```bash
CONTENT_DIR=/tmp/lyracore-wave-defense-content
mkdir -p "$CONTENT_DIR"
cargo +1.93.0 build -p lyracore-importer -p lyracore-package-delta

target/debug/lyracore-importer --dump "$WORLD_DUMP" \
  --world-profile full-world --family creatures > "$CONTENT_DIR/creatures.sql"
target/debug/lyracore-importer --dump "$WORLD_DUMP" --dbc "$CLIENT_DATA" \
  --world-profile full-world --family items > "$CONTENT_DIR/items.sql"

WAVE_DEFENSE_MAPPED_SQL="$CONTENT_DIR" \
WAVE_DEFENSE_SOURCE_SHA=ec4f596146be6467ea93c57397858e329e2db852 \
WAVE_DEFENSE_BASE_OUT="$CONTENT_DIR/base-content.json" \
bun packages/wave-defense/datascripts/defense.ts

target/debug/lyracore-delta-check \
  packages/wave-defense/data/.generated/creatures.json \
  packages/wave-defense/data/.generated/items.json
```

The generator can also read `WAVE_DEFENSE_BASE_CONTENT` directly. That JSON contains `source_sha` and
`tables.game_creature_template` and `tables.game_item_template` arrays, with durable Module column
names and scalar values. `WAVE_DEFENSE_BASE_OUT` saves the mapped JSON for later local checks.
`LYRACORE_CORE_ROOT` overrides the Core checkout, and `LYRACORE_PACKAGES_ROOT` overrides the generated
artifact destination. A missing source row or incompatible Core column stops generation.

The generator clones complete templates and changes only match rules. It never overwrites a base
catalog entry. Stock covers swords, staves, daggers, bows, wands, two-handed maces, shields, cloth,
leather and mail at levels 10, 25, 40 and 55, with plate from level 40. Each tier costs 50 silver,
2 gold, 5 gold or 8 gold. Eight potion choices cost 125 through 2500 copper, and arrows are free.
Every shop item sells for zero. Ordinary equipment and item-use Gates still apply.

`game_npc_vendor` is outside the Package Delta schema. `wave_defense_configure true` installs the
mode's stock idempotently after checking every item and reserved stock identifier. Reconfigure after
an item-family import, which replaces vendor stock.

## Courtyard geometry

The `scarlet-courtyard` importer profile owns only map 189. It imports complete global templates for
all nine launch classes, with no overland population. Its Instance Vmap Slice follows the center
front to the base, has a six-yard exit radius and a two-cell collar.

```bash
target/debug/lyracore-importer --vmap "$CLIENT_DATA" \
  --world-profile scarlet-courtyard
```

The local dry run passed the existing rotation Gate with a 0.1438-yard q25 bound error. Calibration
reads adjacent same-map ADTs within two tiles because the Cathedral placement alone does not pass
calibration.
The output remains confined to the authored courtyard cells. The extracted selection had 36078 WMO
triangles, 25 packed cells, 55 rows and 867335 packed bytes. Its geometry generation was
`5996660338920838971`. All 78 center-route samples had support and headroom, and the direct collision
check was clear. The entry floor was 17.9960 and the base floor was 18.0233.

This is extracted geometry evidence. The private Module check below also tests both offset fronts
and the quartermaster through the configuration Gates. An attended Benilla run still needs to check
camera placement, movement, targeting, shopping and five-person combat.

The private Module check passed with 21 cells of Navigation Coverage. It accepted all three fronts
and the quartermaster, kept one lobby, 51 stock rows and one schedule across repeated configuration,
then removed the schedule on disable. The south, center and north front floor probes returned
18.2097, 17.9960 and 18.0233. A twelve-yard south offset returned 19.9562 and failed the authored
height check, so the active fronts use six-yard offsets.

## Private Module check

The courtyard test starts its own Standalone process on a random loopback port. It imports the
Operator's actual DBC catalogues, selected world families, authored Deltas and courtyard geometry.
It derives Navigation Coverage from that geometry, enables the private collision and navigation
Gates, then configures the mode twice and disables it. It contacts no existing Realm.

```bash
LYRACORE_TEST_IMPORTER="$PWD/target/debug/lyracore-importer" \
LYRACORE_TEST_CLIENT_DATA="$CLIENT_DATA" \
LYRACORE_TEST_WORLD_DUMP="$WORLD_DUMP" \
WAVE_DEFENSE_BASE_CONTENT="$CONTENT_DIR/base-content.json" \
python3 .lyracore/worktrees/packages/.github/run-wave-defense-tests.py . \
  test --test courtyard -- --ignored --nocapture
```

The test prints the path to its geometry, coverage and configuration evidence in a private
`wave-defense-courtyard-*` directory under the system temporary directory. It uses the real source
rows for the four Shaman totems.
The full ordinary loot catalogue is unnecessary for this route check; the mode pays personal gold.
Configuration requires all launch draft spells, full-rank talent spells, the Hunter pet family,
Warlock Imp template 416 and Shaman totem items 5175 through 5178 before it can enable. It also checks
the boss attacks, bot role spells, guaranteed equipment proficiencies and Druid form support spells.

Enablement is an Operator action. It requires a certified single-Shard Realm and refuses any
configuration change while a match row remains, including the result countdown. It creates one
one-second schedule and one empty Package-owned lobby Instance. The lobby and each match exclude
ordinary Scarlet Monastery population. Disabling stops the schedule after every Party has returned.
