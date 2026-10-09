"""Entry point: `python3 scripts/port/lua2rs <command> ...` (or `python3 -m lua2rs` from scripts/port)."""

import os
import sys

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    from lua2rs.cli import main
else:
    from .cli import main

if __name__ == "__main__":
    sys.exit(main())
