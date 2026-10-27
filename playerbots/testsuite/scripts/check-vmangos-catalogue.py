#!/usr/bin/env python3
"""Check the authored quest catalogue against a local, read-only vmangos snapshot."""

import argparse
from pathlib import Path
import re
import sqlite3


def source_objectives(row):
    objectives = []
    for slot in range(1, 5):
        if row[f"ReqItemId{slot}"]:
            objectives.append(("CollectItem", row[f"ReqItemId{slot}"], row[f"ReqItemCount{slot}"]))
        target = row[f"ReqCreatureOrGOId{slot}"]
        if target:
            kind = "UseGameObject" if target < 0 else "KillCreature"
            objectives.append((kind, abs(target), row[f"ReqCreatureOrGOCount{slot}"]))
    return objectives


def check(path):
    package = Path(__file__).resolve().parents[2]
    source = (package / "src/quest_catalog.rs").read_text()
    connection = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA query_only = ON")
    connection.execute("PRAGMA trusted_schema = OFF")
    quests = {row["entry"]: row for row in connection.execute(
        "SELECT * FROM quest_template WHERE patch <= 10 ORDER BY patch"
    )}
    definitions = source.split("const QUESTS:", 1)[1].split("];", 1)[0]
    count = 0
    for definition in definitions.split("QuestDefinition {")[1:]:
        def number(name):
            return int(re.search(rf"{name}: (\d+)", definition)[1])

        entry = number("entry")
        row = quests[entry]
        for field, column in [
            ("min_level", "MinLevel"), ("required_races", "RequiredRaces"),
            ("required_classes", "RequiredClasses"), ("prerequisite_quest", "PrevQuestId"),
        ]:
            assert number(field) == row[column], (entry, field, number(field), row[column])
        for role, suffix in [("start", "questrelation"), ("actual_ender", "involvedrelation")]:
            kind, giver = re.search(rf"{role}: (creature|gameobject)\((\d+)\)", definition).groups()
            givers = {r["id"] for r in connection.execute(
                f"SELECT * FROM {kind}_{suffix} WHERE quest=? AND patch_min<=10 AND patch_max>=10",
                (entry,),
            )}
            assert int(giver) in givers, (entry, role, giver, givers)
        objective_name = re.search(r"objectives: (\w+)", definition)[1]
        expected = []
        if objective_name != "TALK":
            block = source.split(f"const {objective_name}:", 1)[1].split("\n];", 1)[0]
            block = block.split("\n}];", 1)[0]
            for objective in block.split("ObjectiveDefinition {")[1:]:
                kind = re.search(r"kind: CatalogObjectiveKind::(\w+)", objective)[1]
                target = int(re.search(r"target_entry: (\d+)", objective)[1])
                required = int(re.search(r"required_count: (\d+)", objective)[1])
                expected.append((kind, target, required))
                if "ObjectiveExecutor::ProvidedItem" in objective:
                    assert (row["SrcItemId"], row["SrcItemCount"]) == (target, required), entry
        actual = source_objectives(row)
        assert sorted(expected) == sorted(actual), (entry, expected, actual)
        assert row["SpecialFlags"] & 2 == 0, (entry, "requires event credit")
        print(f"quest {entry}: eligibility, relations and objectives match")
        count += 1
    assert count == 22, count
    connection.close()
    print(f"Verified {count} quests at patch 10.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sqlite", type=Path)
    check(parser.parse_args().sqlite)
