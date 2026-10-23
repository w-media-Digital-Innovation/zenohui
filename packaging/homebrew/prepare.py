#!/usr/bin/env python3
"""Generate a Homebrew formula from real source bytes, without Git mutations."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import tarfile
import tomllib
from urllib.parse import urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local", action="store_true", help="Archive the current working files for a local smoke test")
    parser.add_argument("--archive", type=Path, help="Downloaded release source archive (.tar.gz)")
    parser.add_argument("--url", help="HTTPS URL of the exact release source archive")
    parser.add_argument("--output", type=Path, default=Path("target/homebrew/Formula/zenohui.rb"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    if args.local:
        if args.archive or args.url:
            parser.error("--local cannot be combined with --archive or --url")
        version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
        archive = root / "target/homebrew" / f"zenohui-{version}-local.tar.gz"
        archive.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive, "w:gz") as tar:
            for name in ["Cargo.toml", "Cargo.lock", "build.rs", "LICENSE", "README.md", "rustfmt.toml", "src"]:
                tar.add(root / name, arcname=f"zenohui-{version}/{name}")
        url = archive.as_uri()
    else:
        if not args.archive or not args.url:
            parser.error("provide --local, or both --archive and --url")
        archive = args.archive.resolve()
        url = args.url
        if urlsplit(url).scheme != "https" or any(c in url for c in ['"', '\\', '\n', '\r', '#{']):
            parser.error("release URL must be HTTPS and contain no Ruby interpolation or escapes")
        with tarfile.open(archive, "r:gz") as tar:
            manifests = [m for m in tar.getmembers() if m.isfile() and len(Path(m.name).parts) == 2 and Path(m.name).name == "Cargo.toml"]
            if len(manifests) != 1:
                parser.error("archive must contain one top-level source directory with Cargo.toml")
            manifest = tomllib.loads(tar.extractfile(manifests[0]).read().decode())
            if manifest["package"]["name"] != "zenohui":
                parser.error("archive package must be zenohui")
            version = manifest["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        parser.error("unsupported package version")
    sha = hashlib.sha256(archive.read_bytes()).hexdigest()
    formula = (root / "packaging/homebrew/zenohui.rb.in").read_text()
    for token, value in [("SOURCE_URL", url), ("VERSION", version), ("SHA256", sha)]:
        formula = formula.replace(f"@{token}@", json.dumps(value))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(formula)
    print(f"Formula: {args.output.resolve()}")
    print(f"Source: {url}")
    print(f"SHA256: {sha}")
    if args.local:
        print("LOCAL TEST ONLY: contains unpublished working-tree files; do not distribute this formula.")


if __name__ == "__main__":
    main()
