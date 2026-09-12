#!/usr/bin/env python3
"""Export explainshell.db (SQLite) to MessagePack for explainshell-wasm.

Reads the parsed_manpages and mappings tables produced by the explainshell
extraction pipeline and writes a versioned bundle consumed by the
explainshell-data Rust crate (see crates/data/src/lib.rs):

    {"version": 1,
     "manpages": {source: ParsedManpage-as-JSON},
     "mappings": {src: [[dst, score], ...]}}

Usage:
    python scripts/export_wasm_data.py <input.db> <output.msgpack>

The input database is opened read-only. Requires the msgpack package:
    pip install msgpack
"""
from __future__ import annotations

import json
import sqlite3
import sys

try:
    import msgpack
except ImportError:
    sys.stderr.write("ERROR: the msgpack package is required: pip install msgpack\n")
    raise SystemExit(2)

BUNDLE_VERSION = 1

# Mirrors explainshell.help_constants.NO_SYNOPSIS (used by
# ParsedManpage.from_store when the synopsis column is empty).
NO_SYNOPSIS = "no synopsis found"


def _load_manpages(con: sqlite3.Connection) -> dict:
    manpages = {}
    rows = con.execute(
        "SELECT source, name, synopsis, options, aliases, dashless_opts,"
        " subcommands, updated, nested_cmd, extractor, extraction_meta"
        " FROM parsed_manpages"
    )
    for row in rows:
        d = dict(row)
        synopsis = d["synopsis"] or None
        if not synopsis:
            synopsis = NO_SYNOPSIS
        meta_raw = d["extraction_meta"]
        meta = json.loads(meta_raw) if meta_raw else {}
        nested_raw = d["nested_cmd"]
        nested = json.loads(nested_raw) if nested_raw not in (None, "") else False
        manpages[d["source"]] = {
            "source": d["source"],
            "name": d["name"],
            "synopsis": synopsis,
            "options": json.loads(d["options"] or "[]"),
            "aliases": [[a, int(s)] for a, s in json.loads(d["aliases"] or "[]")],
            "dashless_opts": bool(d["dashless_opts"]),
            "subcommands": json.loads(d["subcommands"] or "[]"),
            "updated": bool(d["updated"]),
            "nested_cmd": nested,
            "extractor": d["extractor"],
            "extraction_meta": meta or None,
        }
    return manpages


def _load_mappings(con: sqlite3.Connection) -> dict:
    mappings: dict = {}
    for row in con.execute("SELECT src, dst, score FROM mappings"):
        mappings.setdefault(row["src"], []).append([row["dst"], int(row["score"])])
    return mappings


def export_db(db_path: str, out_path: str) -> None:
    con = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        manpages = _load_manpages(con)
        mappings = _load_mappings(con)
    finally:
        con.close()
    bundle = {"version": BUNDLE_VERSION, "manpages": manpages, "mappings": mappings}
    with open(out_path, "wb") as f:
        msgpack.pack(bundle, f, use_bin_type=True)
    n_mappings = sum(len(v) for v in mappings.values())
    print(f"exported {len(manpages)} manpages, {n_mappings} mappings -> {out_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.stderr.write("usage: export_wasm_data.py <input.db> <output.msgpack>\n")
        raise SystemExit(2)
    export_db(sys.argv[1], sys.argv[2])
