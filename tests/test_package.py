"""
Checks on the installed package itself.
"""

from importlib.metadata import entry_points, version

import ftfy_rs


def test_version_matches_metadata():
    # __version__ is written by hand; the package version comes from Cargo.toml.
    assert ftfy_rs.__version__ == version("ftfy-rs")


def test_cli_entry_point():
    eps = entry_points()
    if hasattr(eps, "select"):
        eps = eps.select(group="console_scripts")
    else:
        # Python 3.9 returns a dict of groups.
        eps = eps.get("console_scripts", [])
    scripts = {ep.name: ep.value for ep in eps}
    assert scripts.get("ftfy-rs") == "ftfy_rs.cli:main"
