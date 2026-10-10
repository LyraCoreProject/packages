# A warm welcome spell

`datascripts/welcome.ts` clones Fireball, spell 133, into Package Spell 6000300 and names it
`A Warm Welcome`. Its other columns and effects stay the same.

Install with `./lyracore packages add example-data`. Run `./lyracore packages apply` with your own
client data configured. It extracts a missing Base Snapshot, builds the Delta locally, and applies it
to your development topology.
The spell has Fireball's effect when cast. An unmodified client has no tooltip for a Package Spell.

Keep the Datascript as source. The generated Delta and its Build Identity stay local under
`data/.generated/` and must not be committed. A copied Package must choose a different Package
Spell ID before it can be installed beside this one.
