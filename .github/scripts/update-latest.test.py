import json
from pathlib import Path
import subprocess
import sys
import unittest

import yaml


class TestPromotion(unittest.TestCase):
    def test_registry_tags(self):
        workflow = Path(__file__).resolve().parents[1] / "workflows/update-latest.yml"
        steps = yaml.safe_load(workflow.read_text())["jobs"]["tag-in-registry"]["steps"]
        update = next(step["run"] for step in steps if step.get("name") == "Update tag")
        record_calls = """pn() {
            if [[ $1 == dist-tag ]]; then
                "$PYTHON" -c 'import json, sys; print(json.dumps(sys.argv[1:]))' "$@"
            fi
        }
        """
        for major, packages in [
            (11, ["pnpm", "@pnpm/exe"]),
            (12, ["pnpm", "@pnpm/exe", "@pnpm/napi"]),
            (13, ["pnpm", "@pnpm/exe", "@pnpm/napi"]),
        ]:
            version = f"{major}.2.0"
            line = f"latest-{major}"
            for tag, tags in [(line, [line]), ("latest", [line, "latest"]), ("custom", [line, "custom"])]:
                with self.subTest(version=version, tag=tag):
                    registry = "https://example.invalid/"
                    calls = subprocess.check_output(
                        ["bash", "-c", record_calls + update],
                        env=dict(VERSION=version, TAG=tag, REGISTRY=registry, NPM_TOKEN="fixture",
                                 PYTHON=sys.executable),
                        text=True,
                    )
                    self.assertCountEqual(
                        map(json.loads, calls.splitlines()),
                        [["dist-tag", "add", f"{package}@{version}", target, f"--registry={registry}"]
                         for package in packages for target in tags],
                    )


if __name__ == "__main__":
    unittest.main()
