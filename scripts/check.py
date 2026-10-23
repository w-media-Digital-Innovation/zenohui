#!/usr/bin/env python3
"""Run the same required quality gate locally and in CI. Never mutates Git."""
import argparse
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use cached dependencies only")
    args = parser.parse_args()
    os.chdir(Path(__file__).resolve().parents[1])
    env = os.environ.copy()
    env["RUSTDOCFLAGS"] = (env.get("RUSTDOCFLAGS", "") + " -D warnings").strip()
    commands = [["git", "diff", "--check"], ["git", "diff", "--cached", "--check"]]
    if not args.offline:
        commands.append(["cargo", "fetch", "--locked"])
    commands.extend([
        ["cargo", "fmt", "--all", "--check"],
        ["cargo", "clippy", "--locked", "--offline", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--locked", "--offline", "--all-targets"],
        ["cargo", "doc", "--locked", "--offline", "--no-deps"],
    ])
    for command in commands:
        print("\n> " + " ".join(command), flush=True)
        result = subprocess.run(command, env=env, check=False)
        if result.returncode:
            return result.returncode
    print("\nQuality gate passed. No commit or push performed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
