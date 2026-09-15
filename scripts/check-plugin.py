#!/usr/bin/env python3
"""Check QFS's shared plugin distribution (stdlib only; run from any directory)."""

import argparse
import json
from pathlib import Path


def check(root):
    errors = []

    def require(condition, message):
        if not condition:
            errors.append(message)

    def read(path):
        try:
            value = json.loads((root / path).read_text())
            if not isinstance(value, dict):
                raise ValueError("expected an object")
            return value
        except (OSError, ValueError) as error:
            errors.append(f"{path}: {error}")
            return {}

    def entry(manifest, path):
        plugins = manifest.get("plugins", [])
        found = [p for p in plugins if isinstance(p, dict) and p.get("name") == "qfs"] if isinstance(plugins, list) else []
        require(len(found) == 1, f"{path}: expected exactly one qfs entry")
        return found[0] if len(found) == 1 else {}

    codex = read("plugins/qfs/.codex-plugin/plugin.json")
    claude = read("plugins/qfs/.claude-plugin/plugin.json")
    marketplace = read(".claude-plugin/marketplace.json")
    cp = entry(marketplace, ".claude-plugin/marketplace.json")
    xp = entry(read(".agents/plugins/marketplace.json"), ".agents/plugins/marketplace.json")
    require(codex.get("name") == claude.get("name") == "qfs", "plugin.json: both host names must be qfs")
    versions = [codex.get("version"), claude.get("version"), marketplace.get("version"), cp.get("version")]
    require(all(isinstance(v, str) and v for v in versions) and all(v == versions[0] for v in versions),
            "plugin/marketplace version fields: all four must match")
    require(codex.get("skills") == "./skills/", ".codex-plugin/plugin.json: skills must point to ./skills/")
    require(cp.get("source") == "./plugins/qfs", ".claude-plugin/marketplace.json: incorrect qfs source")
    require(xp.get("source") == {"source": "local", "path": "./plugins/qfs"},
            ".agents/plugins/marketplace.json: incorrect qfs source")

    skills = root / "plugins/qfs/skills"
    folders = sorted(p for p in skills.iterdir() if p.is_dir()) if skills.is_dir() else []
    actual = {f"./skills/{p.name}" for p in folders}
    listed = cp.get("skills", [])
    require(isinstance(listed, list) and all(isinstance(x, str) for x in listed)
            and len(listed) == len(actual) and set(listed) == actual,
            ".claude-plugin/marketplace.json: skills must list each actual skill exactly once")
    require("./skills/qfs" in actual, "plugins/qfs/skills/qfs: base skill is missing")

    generated = {}
    cookbook = root / "docs/cookbook"
    if cookbook.is_dir():
        for source in sorted(cookbook.glob("*.md")):
            try:
                content = source.read_text()
            except OSError as error:
                errors.append(f"{source.relative_to(root)}: {error}")
                continue
            if not content.startswith("---\n") or "\n---\n" not in content[4:]:
                continue
            front, body = content[4:].split("\n---\n", 1)
            fields = {}
            for line in front.splitlines():
                key, separator, value = line.partition(":")
                if separator:
                    fields[key] = value.strip()
            name = fields.get("skill_name")
            description = fields.get("skill_description")
            if name and description:
                generated[name] = (
                    f"---\nname: {name}\ndescription: {description}\n---\n\n"
                    f"{body.lstrip(chr(10)).rstrip()}\n"
                )

    for folder in folders:
        path = folder / "SKILL.md"
        try:
            text = path.read_text()
            front = text.split("---", 2)[1] if text.startswith("---\n") else ""
            require(f"\nname: {folder.name}\n" in front and "\ndescription: " in front,
                    f"{path.relative_to(root)}: name/description frontmatter missing or mismatched")
            expected = generated.get(folder.name)
            if expected is not None:
                require(text == expected, f"{path.relative_to(root)}: generated skill is out of date")
        except OSError as error:
            errors.append(f"{path.relative_to(root)}: {error}")
        link = root / ".claude/skills" / folder.name
        require(link.is_symlink() and link.resolve() == folder.resolve(),
                f".claude/skills/{folder.name}: missing or incorrect symlink")
    if cookbook.is_dir():
        require(set(generated) == {p.name for p in folders if p.name != "qfs"},
                "docs/cookbook and generated skill directories must match exactly")
    return errors


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    errors = check(parser.parse_args().root.resolve())
    for error in errors:
        print(f"DRIFT: {error}")
    if errors:
        raise SystemExit(1)
    print("QFS plugin distribution is in sync (both hosts, versions, skills and symlinks).")
