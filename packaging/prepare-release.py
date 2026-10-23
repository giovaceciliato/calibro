#!/usr/bin/env python3
"""Validate the desktop downloads and generate release notes with exact asset links."""

import argparse
import hashlib
import re
from pathlib import Path
from urllib.parse import quote


def prepare(tag: str, repository: str, assets: Path, template: Path) -> str:
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", repository):
        raise ValueError("Repository must be owner/name")
    if not tag.startswith("v") or not re.fullmatch(r"[\w.+/-]+", tag):
        raise ValueError("Expected a version tag starting with v")
    choices = [
        ("Windows 10/11 · executable", "calibro-windows-x86_64.exe", "Download and run the .exe"),
        ("Windows 10/11 · ZIP", "calibro-windows-x86_64.zip", "Extract, then run calibro.exe"),
        ("Fedora 40+ · RPM", "calibro-*.rpm", "Open the RPM with your package installer"),
        ("Linux / Bazzite · app-menu installer", "calibro-[0-9]*-linux-x86_64.tar.gz", "Extract, then run ./install.sh"),
        ("macOS · Apple Silicon", "calibro-macos-arm64.tar.gz", "Extract, then run the calibro executable"),
        ("Linux · binary archive", "calibro-linux-x86_64.tar.gz", "Extract, then run ./calibro"),
    ]
    rows = []
    downloads = []
    base = f"https://github.com/{repository}/releases/download/{quote(tag, safe='')}"
    # Check every platform before creating metadata or publishing a partial release.
    for platform, pattern, instruction in choices:
        matches = sorted(path for path in assets.glob(pattern) if path.is_file())
        if len(matches) != 1 or matches[0].stat().st_size == 0:
            raise ValueError(f"Expected exactly one nonempty {pattern}, found {len(matches)}")
        path = matches[0]
        downloads.append(path)
        url = f"{base}/{quote(path.name, safe='')}"
        rows.append(f"| {platform} | [{path.name}]({url}) | {instruction} |")
    checksums = []
    for path in sorted(downloads):
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        checksums.append(f"{digest}  {path.name}\n")
    (assets / "SHA256SUMS.txt").write_text("".join(checksums), encoding="utf-8")
    table = "\n".join([
        "## Downloads", "", "| System | Download | How to launch |",
        "|---|---|---|", *rows, "",
        f"[Verify downloads with SHA-256 checksums]({base}/SHA256SUMS.txt)", "",
        "The files are also listed under **Assets** below. GitHub's source-code archives are not desktop apps.", "",
    ])
    body = template.read_text(encoding="utf-8")
    body, count = re.subn(r"## Downloads\n.*?(?=\n## |\Z)", lambda _: table, body, count=1, flags=re.S)
    if count != 1:
        raise ValueError("Release notes must contain a Downloads section")
    return body.replace("](docs/", f"](https://github.com/{repository}/blob/{quote(tag, safe='')}/docs/")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--assets-dir", type=Path, required=True)
    parser.add_argument("--template", type=Path, default=Path("RELEASE_NOTES.md"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    body = prepare(args.tag, args.repository, args.assets_dir, args.template)
    args.output.write_text(body, encoding="utf-8")
    print(f"Release notes ready: {args.output} (six desktop downloads + checksums)")


if __name__ == "__main__":
    main()
