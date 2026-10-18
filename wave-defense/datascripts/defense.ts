import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const core = resolve(process.env.LYRACORE_CORE_ROOT ?? process.cwd());
const input = process.env.WAVE_DEFENSE_BASE_CONTENT;
const schema = await readFile(join(core, "crates/lyracore-package-delta/src/schema.rs"), "utf8");
const mappedSql = process.env.WAVE_DEFENSE_MAPPED_SQL;
if (!input && !mappedSql) throw new Error("Set WAVE_DEFENSE_BASE_CONTENT to offline Module JSON or WAVE_DEFENSE_MAPPED_SQL to mapped SQL files");
const base = input ? JSON.parse(await readFile(input, "utf8")) : await prepareContent(mappedSql!);
if (!/^[a-f0-9]{40}$/.test(base.source_sha ?? "")) throw new Error("Content input requires its upstream source_sha");
if (process.env.WAVE_DEFENSE_BASE_OUT) {
  await writeFile(process.env.WAVE_DEFENSE_BASE_OUT, JSON.stringify(base));
}
const sourceHash = createHash("sha256").update(await readFile(new URL(import.meta.url))).digest("hex");
const output = join(process.env.LYRACORE_PACKAGES_ROOT ?? join(core, "packages"), "wave-defense/data/.generated");
const tables = base.tables;
const creatureBase = 15_189_001;
const itemBase = 7_189_000;

type Row = Record<string, string | number | boolean>;
type Claim = { table: string; key: { entry: number }; operation: "insert"; fields: Record<string, { type: string; value: string | number | boolean }> };

function columns(table: string): [string, string][] {
  const declaration = table === "game_creature_template" ? "CREATURE_TEMPLATE_COLUMNS" : "ITEM_COLUMNS";
  const block = schema.match(new RegExp(`const ${declaration}:.*?= &\\[([\\s\\S]*?)\\n\\];`))?.[1];
  if (!block) throw new Error(`Core has no ${declaration}`);
  return [...block.matchAll(/column\("([^"]+)", FieldType::(\w+)\)/g)].map((column) => [column[1]!, column[2] === "Str" ? "string" : column[2]!.toLowerCase()]);
}

async function prepareContent(directory: string) {
  const tables: Record<string, Row[]> = {};
  for (const [table, file] of [["game_creature_template", "creatures.sql"], ["game_item_template", "items.sql"]]) {
    const types = Object.fromEntries([["entry", "u32"], ...columns(table!)]);
    const text = await readFile(join(directory, file!), "utf8");
    const rows: Row[] = [];
    for (const statement of text.matchAll(new RegExp(`INSERT INTO ${table} \\(([^)]+)\\) VALUES ([\\s\\S]*?);\\n`, "g"))) {
      const names = statement[1]!.split(",");
      let row: string[] = [];
      let value = "";
      let quoted = false;
      let escaped = false;
      const values = statement[2]!;
      for (let index = 0; index < values.length; index++) {
        const char = values[index]!;
        if (escaped) { value += char; escaped = false; continue; }
        if (quoted && char === "\\") { escaped = true; continue; }
        if (char === "'") {
          if (quoted && values[index + 1] === "'") { value += "'"; index++; } else { quoted = !quoted; }
          continue;
        }
        if (!quoted && char === "(") { row = []; value = ""; continue; }
        if (!quoted && char === ",") { if (row.length || value) { row.push(value); value = ""; } continue; }
        if (!quoted && char === ")") {
          row.push(value);
          rows.push(Object.fromEntries(names.map((name, i) => [name, types[name] === "string" || types[name] === "u64" ? row[i]! : types[name] === "bool" ? row[i]!.trim() === "true" : Number(row[i])])));
          value = "";
          row = [];
          continue;
        }
        value += char;
      }
    }
    tables[table!] = rows;
  }
  return { source_sha: process.env.WAVE_DEFENSE_SOURCE_SHA, tables };
}

function clone(table: string, sourceId: number, entry: number, changes: Row): Claim {
  const row = tables?.[table]?.find((row: Row) => row.entry === sourceId);
  if (!row) throw new Error(`Missing imported ${table} entry ${sourceId}`);
  const authored = { ...row, ...changes };
  const fields: Claim["fields"] = {};
  for (const [name, type] of columns(table)) {
    const value = authored[name];
    if (value === undefined) throw new Error(`Missing imported ${table}.${name} for ${sourceId}`);
    if (type === "string" ? typeof value !== "string" : type === "bool" ? typeof value !== "boolean" : type === "u64" ? !/^\d+$/.test(String(value)) : typeof value !== "number" || !Number.isFinite(value)) {
      throw new Error(`Invalid ${type} at ${table}.${name} for ${sourceId}`);
    }
    fields[name] = { type, value: type === "u64" ? String(value) : value };
  }
  return { table, key: { entry }, operation: "insert", fields };
}

const creatures = [
  clone("game_creature_template", 1525, creatureBase, { name: "Courtyard Invader", subname: "", level: 1, max_level: 1, health: 100, max_level_health: 100, npc_flags: 0, unit_flags: 0, rank: 0, aggro_range: 45, money_min: 0, money_max: 0, pickpocket_loot_id: 0, skin_loot_id: 0 }),
  clone("game_creature_template", 1530, creatureBase + 1, { name: "Courtyard Warlord", subname: "", level: 1, max_level: 1, health: 1000, max_level_health: 1000, npc_flags: 0, unit_flags: 0, rank: 1, scale: 1.5, aggro_range: 45, money_min: 0, money_max: 0, pickpocket_loot_id: 0, skin_loot_id: 0 }),
  clone("game_creature_template", 68, creatureBase + 2, { name: "Defense Quartermaster", subname: "Match supplies", level: 60, max_level: 60, health: 10000, max_level_health: 10000, faction_template: 35, npc_flags: 128, unit_flags: 2, rank: 0, aggro_range: 0, money_min: 0, money_max: 0, pickpocket_loot_id: 0, skin_loot_id: 0 }),
  clone("game_creature_template", 299, creatureBase + 3, { name: "Defense Wolf", subname: "", level: 1, max_level: 1, health: 100, max_level_health: 100, npc_flags: 0, unit_flags: 0, rank: 0, aggro_range: 0, money_min: 0, money_max: 0, pickpocket_loot_id: 0, skin_loot_id: 0 }),
];
const tiers = [
  { level: 10, price: 5_000, entries: [4765, 1933, 1287, 3036, 5071, 6205, 3651, 2578, 1211, 2273] },
  { level: 25, price: 20_000, entries: [753, 1155, 3336, 15284, 8184, 1976, 4064, 2234, 14570, 914] },
  { level: 40, price: 50_000, entries: [7943, 7713, 10761, 13020, 13064, 9423, 7726, 10020, 14601, 1715, 8157] },
  { level: 55, price: 80_000, entries: [18463, 13372, 12582, 13175, 13534, 13167, 13375, 14138, 15059, 14798, 12422] },
];
const items: Claim[] = [];
for (const tier of tiers) {
  for (const source of tier.entries) {
    items.push(clone("game_item_template", source, itemBase + items.length, {
      buy_price: tier.price, sell_price: 0, required_level: tier.level, buy_count: 1,
      required_skill: 0, required_skill_rank: 0, required_reputation_faction: 0, required_reputation_rank: 0,
      start_quest: 0, page_text: 0, bonding: 0,
    }));
  }
}
for (const [source, price] of [[118, 125], [929, 500], [1710, 1000], [3928, 2000], [2455, 250], [3385, 600], [3827, 1200], [13443, 2500], [2512, 0]]) {
  items.push(clone("game_item_template", source!, itemBase + items.length, {
    buy_price: price!, sell_price: 0, buy_count: source === 2512 ? 200 : 1, bonding: 0,
  }));
}
await mkdir(output, { recursive: true });
for (const [family, claims] of [["creatures", creatures], ["items", items]] as const) {
  const path = join(output, `${family}.json`);
  await writeFile(path, JSON.stringify({ version: 1, package: "wave-defense", source_hash: sourceHash, claims }, null, 2) + "\n");
  console.log(`${family}: ${claims.length} owned inserts at ${path}`);
}
