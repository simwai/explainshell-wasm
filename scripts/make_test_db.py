#!/usr/bin/env python3
"""Create a minimal explainshell.db fixture for CI/tests.

Writes test/fixtures/test.db with a handful of hand-authored manpages
(tar, git, git-commit, sudo) in the same schema the real extraction
pipeline produces, so export_wasm_data.py and the Rust data crate can be
tested without running the full LLM extraction pipeline.

Usage:
    python scripts/make_test_db.py [output.db]
"""
from __future__ import annotations

import json
import os
import sqlite3
import sys

SCHEMA = """
CREATE TABLE IF NOT EXISTS parsed_manpages (
    source        TEXT    PRIMARY KEY,
    name          TEXT    NOT NULL,
    synopsis      TEXT,
    options       TEXT    NOT NULL DEFAULT '[]',
    aliases       TEXT    NOT NULL DEFAULT '[]',
    dashless_opts INTEGER NOT NULL DEFAULT 0,
    subcommands   TEXT    NOT NULL DEFAULT '[]',
    updated       INTEGER NOT NULL DEFAULT 0,
    nested_cmd    TEXT    NOT NULL DEFAULT 'false',
    extractor     TEXT,
    extraction_meta TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS mappings (
    src   TEXT    NOT NULL,
    dst   TEXT    NOT NULL,
    score INTEGER NOT NULL,
    PRIMARY KEY (src, dst)
);
"""


def _opt(text, short=None, long=None, has_argument=False, positional=None,
         prefix=None, nested_cmd=False):
    return {
        "text": text,
        "short": short or [],
        "long": long or [],
        "has_argument": has_argument,
        "positional": positional,
        "prefix": prefix,
        "nested_cmd": nested_cmd,
        "meta": None,
    }


FIXTURES = [
    {
        "source": "ubuntu/26.04/1/tar.1.gz",
        "name": "tar",
        "synopsis": "manipulate tape archives",
        "options": [
            _opt("verbose output", short=["-v"], long=["--verbose"]),
            _opt("extract files", short=["-x"], long=["--extract"]),
            _opt("archive file to use", short=["-f"], long=["--file"],
                 has_argument=True),
            _opt("archive name", positional="archive"),
        ],
        "aliases": [["tar", 10]],
        "subcommands": [],
        "nested_cmd": False,
        "mappings": [("tar", 10)],
    },
    {
        "source": "ubuntu/26.04/1/git.1.gz",
        "name": "git",
        "synopsis": "the stupid content tracker",
        "options": [
            _opt("show version", long=["--version"]),
            _opt("repository path", long=["--git-dir"], has_argument=True),
        ],
        "aliases": [["git", 10]],
        "subcommands": ["commit", "push"],
        "nested_cmd": False,
        "mappings": [("git", 10)],
    },
    {
        "source": "ubuntu/26.04/1/git-commit.1.gz",
        "name": "git-commit",
        "synopsis": "record changes to the repository",
        "options": [
            _opt("commit message", short=["-m"], long=["--message"],
                 has_argument=True),
            _opt("stage all", short=["-a"], long=["--all"]),
            _opt("files to commit", positional="pathspec"),
        ],
        "aliases": [["git-commit", 10]],
        "subcommands": [],
        "nested_cmd": False,
        "mappings": [("git-commit", 10), ("git commit", 1)],
    },
    {
        "source": "ubuntu/26.04/8/sudo.8.gz",
        "name": "sudo",
        "synopsis": "execute a command as another user",
        "options": [
            _opt("run as user", short=["-u"], long=["--user"],
                 has_argument=True),
            _opt("command to run", positional="command"),
        ],
        "aliases": [["sudo", 10]],
        "subcommands": [],
        "nested_cmd": True,
        "mappings": [("sudo", 10)],
    },
]


def make_db(path: str) -> None:
    if os.path.exists(path):
        os.remove(path)
    con = sqlite3.connect(path)
    try:
        con.executescript(SCHEMA)
        for fx in FIXTURES:
            con.execute(
                "INSERT INTO parsed_manpages(source, name, synopsis, options,"
                " aliases, dashless_opts, subcommands, updated, nested_cmd,"
                " extractor, extraction_meta)"
                " VALUES (?,?,?,?,?,?,?,?,?,?,?)",
                (
                    fx["source"],
                    fx["name"],
                    fx["synopsis"],
                    json.dumps(fx["options"]),
                    json.dumps(fx["aliases"]),
                    0,
                    json.dumps(fx["subcommands"]),
                    0,
                    json.dumps(fx["nested_cmd"]),
                    "fixture",
                    json.dumps({}),
                ),
            )
            for src, score in fx["mappings"]:
                con.execute(
                    "INSERT INTO mappings(src, dst, score) VALUES (?,?,?)",
                    (src, fx["source"], score),
                )
        con.commit()
    finally:
        con.close()
    print(f"wrote {len(FIXTURES)} fixture manpages -> {path}")


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "test/fixtures/test.db"
    make_db(out)
