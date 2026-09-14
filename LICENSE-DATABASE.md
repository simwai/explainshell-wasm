# Database License Notice

The `explainshell.data.msgpack` bundle distributed with this package is derived from the **explainshell database** published at <https://github.com/idank/explainshell/releases/tag/db-latest>.

## Upstream Licenses

The database contains the (near-)verbatim text of tens of thousands of man pages extracted from **Ubuntu** and **Arch Linux** packages. Each man page retains its own upstream license, held by its respective authors. The GPL-3.0-or-later license covering the explainshell **code** does **not** cover this database content.

Common licenses found in the corpus include (non-exhaustive):

- **GPL-2.0 / GPL-3.0** — GNU coreutils, bash, and many system utilities
- **BSD-2-Clause / BSD-3-Clause** — Various BSD-derived tools
- **MIT** — Modern utilities and libraries
- **Apache-2.0** — Some Apache projects
- **Custom / Public Domain** — e.g., `man-pages` project pages, `mandoc` output

The `source` field in each manpage entry (format: `distro/release/section/name.section.gz`, e.g., `ubuntu/26.04/1/tar.1.gz`) identifies the originating package and distribution, enabling license tracing.

## Redistribution Terms

This package redistributes the extracted option data (synopsis, options, aliases, mappings) as a **MessagePack bundle** (`explainshell.data.msgpack`), not the full manpage text. The extraction transforms the original roff/markdown into structured JSON.

If you redistribute this package (or a derivative), you **must**:

1. **Preserve the `source` field** — it identifies the upstream manpage origin for license compliance.
2. **Perform your own compliance review** — verify that your use case and distribution model are compatible with the licenses of the individual man pages you expose.
3. **Include this notice** — retain this `LICENSE-DATABASE.md` (or equivalent) in your distribution.

The explainshell project maintainers **cannot grant blanket redistribution rights** for the database content, as they do not hold copyright on the upstream man pages.

## Source

- Database releases: <https://github.com/idank/explainshell/releases/tag/db-latest>
- explainshell code (GPL-3.0-or-later): <https://github.com/idank/explainshell>
- Manpage sources: Ubuntu manpages archive, Arch Linux packages, manned.org (via explainshell-manpages submodule)

## Build-Time Download

This package's build script (`scripts/build-wasm.mjs`) automatically downloads the latest `explainshell-*.db.zst` asset from the `db-latest` GitHub release, verifies its SHA256, decompresses it, and exports the data bundle. The downloaded SQLite database is cached locally in `.explainshell-cache/` (gitignored) and is **not** included in the npm package — only the exported `dist/explainshell.data.msgpack` is published.

To build offline, populate `.explainshell-cache/` manually or set `EXPLAINSHELL_DB` to a local database path.