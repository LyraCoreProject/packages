#!/usr/bin/env python3
"""Build example-data against Core's synthetic Base Snapshot, outside the collection."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def check(core: Path) -> None:
    collection = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "datascripts").symlink_to(core / "datascripts", target_is_directory=True)
        package = root / "packages/example-data"
        shutil.copytree(collection / "example-data/datascripts", package / "datascripts")
        source = package / "datascripts/welcome.ts"
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", prefix=".example-data-", dir=core / "datascripts"
        ) as config:
            json.dump({"extends": "./tsconfig.json", "files": [str(source)]}, config)
            config.flush()
            subprocess.run(
                ["bun", "./node_modules/typescript/bin/tsc", "--noEmit", "--project", config.name],
                cwd=core / "datascripts", check=True,
            )
        subprocess.run(
            ["bun", "run", str(source)], check=True,
            env={**os.environ,
                 "LYRACORE_BASE_SNAPSHOT": str(core / "datascripts/tests/fixtures/base-snapshot.json"),
                 "LYRACORE_PACKAGES_ROOT": str(root / "packages")},
        )
        artifact = package / "data/.generated/spell.json"
        delta = json.loads(artifact.read_text())
        assert delta["package"] == "example-data"
        spells = [claim for claim in delta["claims"] if claim["table"] == "game_spell"]
        assert len(spells) == 1
        assert spells[0]["operation"] == "insert"
        assert spells[0]["key"] == {"spell_id": 6000300}
        assert spells[0]["fields"]["name"]["value"] == "A Warm Welcome"
        subprocess.run(
            ["cargo", "run", "--locked", "--quiet", "-p", "lyracore-package-delta",
             "--bin", "lyracore-delta-check", "--", str(artifact)],
            cwd=core, check=True,
        )


if __name__ == "__main__":
    check(Path(sys.argv[1]).resolve())
