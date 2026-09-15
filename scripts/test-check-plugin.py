#!/usr/bin/env python3
"""Negative distribution fixtures; no credentials, host installation or network."""

import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("check_plugin", ROOT / "scripts/check-plugin.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


class PluginDistribution(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for directory in ("plugins", ".agents", ".claude-plugin", ".claude/skills", "docs/cookbook"):
            shutil.copytree(ROOT / directory, self.root / directory, symlinks=True)

    def mutate(self, file, transform):
        path = self.root / file
        value = json.loads(path.read_text())
        transform(value)
        path.write_text(json.dumps(value))

    def test_valid(self):
        self.assertEqual(CHECKER.check(self.root), [])

    def test_codex_source_missing(self):
        self.mutate(".agents/plugins/marketplace.json", lambda m: m["plugins"][0]["source"].update(path="./missing"))
        self.assertTrue(any("incorrect qfs source" in e for e in CHECKER.check(self.root)))

    def test_codex_skills_path_wrong(self):
        self.mutate("plugins/qfs/.codex-plugin/plugin.json", lambda m: m.update(skills="./missing/"))
        self.assertTrue(any("skills must point" in e for e in CHECKER.check(self.root)))

    def test_version_mismatch(self):
        self.mutate("plugins/qfs/.codex-plugin/plugin.json", lambda m: m.update(version="0.0.0"))
        self.assertTrue(any("all four" in e for e in CHECKER.check(self.root)))

    def test_base_missing(self):
        shutil.rmtree(self.root / "plugins/qfs/skills/qfs")
        self.assertTrue(any("base skill" in e for e in CHECKER.check(self.root)))

    def test_duplicate_registration(self):
        self.mutate(".claude-plugin/marketplace.json", lambda m: m["plugins"][0]["skills"].append("./skills/qfs"))
        self.assertTrue(any("exactly once" in e for e in CHECKER.check(self.root)))

    def test_extra_registration(self):
        self.mutate(".claude-plugin/marketplace.json", lambda m: m["plugins"][0]["skills"].append("./skills/nope"))
        self.assertTrue(any("exactly once" in e for e in CHECKER.check(self.root)))

    def test_generated_body_drift(self):
        path = self.root / "plugins/qfs/skills/qfs-slack/SKILL.md"
        path.write_text(path.read_text().replace("A Slack channel is", "A Slack stream is", 1))
        self.assertTrue(any("generated skill is out of date" in e for e in CHECKER.check(self.root)))

    def test_missing_skill_body(self):
        (self.root / "plugins/qfs/skills/qfs-slack/SKILL.md").unlink()
        self.assertTrue(any("qfs-slack/SKILL.md" in e for e in CHECKER.check(self.root)))

    def test_malformed_manifest(self):
        (self.root / "plugins/qfs/.codex-plugin/plugin.json").write_text("{")
        self.assertTrue(any(".codex-plugin/plugin.json" in e for e in CHECKER.check(self.root)))

    def test_base_symlink_missing(self):
        (self.root / ".claude/skills/qfs").unlink()
        self.assertTrue(any(".claude/skills/qfs" in e for e in CHECKER.check(self.root)))


if __name__ == "__main__":
    unittest.main()
