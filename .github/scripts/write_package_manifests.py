#!/usr/bin/env python3
"""Write the Homebrew formula and Scoop manifest for a release.

Usage (env: VERSION, MACOS_ARM, MACOS_X86, LINUX_ARM, LINUX_X86, WIN_X86,
WIN_ARM):

    write_package_manifests.py OUT_DIR    # -> OUT_DIR/trelloc.rb, trelloc.json
"""

import json
import os
import re
import sys
from pathlib import Path

REPO = "mulhamna/trello-commands"
DESC = "Unofficial Trello CLI and MCP server"


def url(version: str, asset: str) -> str:
    return f"https://github.com/{REPO}/releases/download/v{version}/{asset}"


def formula(e: dict) -> str:
    v = e["VERSION"]

    def block(asset: str, sha: str) -> str:
        return f'      url "{url(v, asset)}"\n      sha256 "{sha}"'

    return f'''class Trelloc < Formula
  desc "{DESC}"
  homepage "https://github.com/{REPO}"
  version "{v}"
  license "MIT"

  on_macos do
    on_arm do
{block("trelloc-macos-aarch64.tar.gz", e["MACOS_ARM"])}
    end
    on_intel do
{block("trelloc-macos-x86_64.tar.gz", e["MACOS_X86"])}
    end
  end

  on_linux do
    on_arm do
{block("trelloc-linux-aarch64.tar.gz", e["LINUX_ARM"])}
    end
    on_intel do
{block("trelloc-linux-x86_64.tar.gz", e["LINUX_X86"])}
    end
  end

  def install
    bin.install Dir["trelloc-*"].first => "trelloc"
  end

  test do
    assert_match version.to_s, shell_output("#{{bin}}/trelloc --version")
  end
end
'''


def scoop(e: dict) -> str:
    v = e["VERSION"]
    arch = {
        "64bit": ("trelloc-windows-x86_64", e["WIN_X86"]),
        "arm64": ("trelloc-windows-aarch64", e["WIN_ARM"]),
    }
    manifest = {
        "version": v,
        "description": DESC,
        "homepage": f"https://github.com/{REPO}",
        "license": "MIT",
        "architecture": {
            k: {"url": url(v, f"{n}.zip"), "hash": sha, "bin": [[f"{n}.exe", "trelloc"]]}
            for k, (n, sha) in arch.items()
        },
        "checkver": "github",
        "autoupdate": {
            "architecture": {
                k: {"url": f"https://github.com/{REPO}/releases/download/v$version/{n}.zip"}
                for k, (n, _) in arch.items()
            }
        },
    }
    return json.dumps(manifest, indent=2) + "\n"


def main() -> None:
    e = dict(os.environ)
    missing = [k for k in ("VERSION",) if not e.get(k)]
    if missing:
        raise SystemExit(f"missing env: {missing}")
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    for key in ("MACOS_ARM", "MACOS_X86", "LINUX_ARM", "LINUX_X86", "WIN_X86", "WIN_ARM"):
        if not re.fullmatch(r"[0-9a-f]{64}", e.get(key, "")):
            raise SystemExit(f"{key} is not a sha256: {e.get(key)!r}")
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    (out / "trelloc.rb").write_text(formula(e))
    (out / "trelloc.json").write_text(scoop(e))
    print(f"wrote {out}/trelloc.rb and {out}/trelloc.json")


if __name__ == "__main__":
    main()
