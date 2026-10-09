"""Shared helpers for the P4 script tests (python3 -m unittest discover -s scripts/port/tests)."""

import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = os.path.dirname(HERE)
FIXTURES = os.path.join(HERE, "fixtures")
if PORT not in sys.path:
    sys.path.insert(0, PORT)

import pobpaths  # noqa: E402


def real_pob():
    """The PathOfBuilding checkout at the pin, or None (tests needing it are skipped)."""
    try:
        return pobpaths.find_pob(None)
    except SystemExit:
        return None


def needs_pob(test):
    return unittest.skipIf(real_pob() is None, "PathOfBuilding checkout not found")(test)


def fixture(*parts):
    return os.path.join(FIXTURES, *parts)
