#!/usr/bin/env python3
"""Copy the repository's own docs into the book, pointing their relative links at GitHub.

ARCHITECTURE.md, DECISIONS.md, ROADMAP.md and CONTRIBUTING.md link to files next to them
(ADRs, research notes, source). Inside the book those files don't exist, so each relative
link becomes a link to the file on GitHub. Run before `mdbook build`; CI does.
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BLOB = "https://github.com/PndaMan/delune/blob/main/"
TREE = "https://github.com/PndaMan/delune/tree/main/"
PAGES = {
    "architecture": "docs/ARCHITECTURE.md",
    "decisions": "docs/DECISIONS.md",
    "roadmap": "docs/ROADMAP.md",
    "contributing": "CONTRIBUTING.md",
}
LINK = re.compile(r"\]\(([^)\s]+)\)")


def rewrite(text: str, source: Path) -> str:
    def fix(match: re.Match) -> str:
        target = match.group(1)
        if re.match(r"^(https?:|mailto:|#)", target):
            return match.group(0)
        path, _, anchor = target.partition("#")
        resolved = (source.parent / path).resolve().relative_to(ROOT)
        base = TREE if (ROOT / resolved).is_dir() else BLOB
        return f"]({base}{resolved.as_posix()}{'#' + anchor if anchor else ''})"

    return LINK.sub(fix, text)


def main() -> None:
    out = ROOT / "docs/book/src/project"
    out.mkdir(parents=True, exist_ok=True)
    for name, relative in PAGES.items():
        source = ROOT / relative
        header = f"<!-- Generated from {relative} by scripts/docs-project-pages.py; edit that file instead. -->\n\n"
        (out / f"{name}.md").write_text(header + rewrite(source.read_text(), source))


if __name__ == "__main__":
    main()
