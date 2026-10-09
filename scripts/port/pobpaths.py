"""Path helpers shared by the P4 port scripts (Python 3 stdlib only).

The PathOfBuilding checkout is gitignored, so git worktrees do not contain it.
`find_pob()` resolves it in this order: explicit argument, $POB_DIR, `<repo>/.PathOfBuilding`,
then `<main checkout>/.PathOfBuilding` (found through `git rev-parse --git-common-dir`).
"""

import os
import subprocess

# The upstream pin the port targets. It is the fallback for checkout_pin() when the checkout
# is not a git repository, and the pin `provenance.py scope --emit` writes. Update it after
# `provenance.py bump --apply` has moved the whole port to a new upstream commit.
PIN = "16de4b82d57f1c0de6eb40f37143c32d4da36a02"
PIN_SHORT = PIN[:9]


def checkout_pin(pob):
    """The commit the PathOfBuilding checkout `pob` has checked out (its HEAD), or PIN when `pob`
    is not a git repository. Marker hashes are verified against the files of this checkout, so
    @pob-file pins are compared with this commit too."""
    try:
        out = subprocess.run(["git", "rev-parse", "--verify", "-q", "HEAD"], cwd=pob,
                             capture_output=True, text=True, check=True).stdout.strip()
        top = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=pob,
                             capture_output=True, text=True, check=True).stdout.strip()
        if out and os.path.realpath(top) == os.path.realpath(pob):
            return out
    except (OSError, subprocess.CalledProcessError):
        pass
    return PIN


def repo_root(start=None):
    """Root of the git working tree containing this script (or `start`)."""
    here = start or os.path.dirname(os.path.abspath(__file__))
    try:
        out = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=here,
                             capture_output=True, text=True, check=True).stdout.strip()
        if out:
            return out
    except (OSError, subprocess.CalledProcessError):
        pass
    return os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))


def main_checkout(start=None):
    """Root of the main checkout (differs from repo_root() inside a git worktree)."""
    here = start or os.path.dirname(os.path.abspath(__file__))
    try:
        common = subprocess.run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=here,
                                capture_output=True, text=True, check=True).stdout.strip()
        if common:
            return os.path.dirname(common.rstrip("/"))
    except (OSError, subprocess.CalledProcessError):
        pass
    return repo_root(start)


def find_pob(explicit=None):
    cands = []
    if explicit:
        cands.append(explicit)
    if os.environ.get("POB_DIR"):
        cands.append(os.environ["POB_DIR"])
    cands.append(os.path.join(repo_root(), ".PathOfBuilding"))
    cands.append(os.path.join(main_checkout(), ".PathOfBuilding"))
    for c in cands:
        if os.path.isdir(os.path.join(c, "src")):
            return os.path.abspath(c)
    raise SystemExit("PathOfBuilding checkout not found (tried: %s); pass --pob or set POB_DIR" % ", ".join(cands))


def docs_port(root=None):
    return os.path.join(root or repo_root(), "docs", "port")
