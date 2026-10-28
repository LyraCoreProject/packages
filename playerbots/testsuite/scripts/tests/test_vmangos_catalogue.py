import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "vmangos_catalogue", Path(__file__).resolve().parents[1] / "check-vmangos-catalogue.py"
)
catalogue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(catalogue)


class Objectives(unittest.TestCase):
    def test_creature_and_gameobject_targets_use_their_own_kind(self):
        row = {
            f"{column}{slot}": 0
            for column in ["ReqItemId", "ReqItemCount", "ReqCreatureOrGOId", "ReqCreatureOrGOCount"]
            for slot in range(1, 5)
        }
        row.update(ReqCreatureOrGOId1=123, ReqCreatureOrGOCount1=2,
                   ReqCreatureOrGOId2=-456, ReqCreatureOrGOCount2=3)
        self.assertEqual(catalogue.source_objectives(row), [
            ("KillCreature", 123, 2), ("UseGameObject", 456, 3),
        ])
