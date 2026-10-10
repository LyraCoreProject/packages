# A warm welcome spell

`datascripts/welcome.ts` clones Fireball, spell 133, into Package Spell 6000300 and names it
`A Warm Welcome`. Its other columns and effects stay the same.

Install with `./lyracore packages add example-data`. With a Base Snapshot from your own client
data, run `./lyracore packages build`, then use the normal base import to apply the Package Delta.
The spell has Fireball's effect when cast. An unmodified client has no tooltip for a Package Spell.

Keep the Datascript as source. The generated Delta and its Build Identity stay local under
`data/.generated/` and must not be committed. A copied Package must choose a different Package
Spell ID before it can be installed beside this one.
