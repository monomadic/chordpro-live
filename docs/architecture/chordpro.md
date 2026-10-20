# ChordPro parsing

`cpl-song` owns source parsing and the platform-independent `Song` model.
Chord markers split a lyric line into segments so each chord belongs to the
lyric text immediately after it. Original spaces in each segment are kept.

The current parser is an initial slice, **not ChordPro 6 conformant**. It
handles common metadata, verse/chorus/bridge sections, verbatim tab sections,
UTF-8 BOM, CRLF, and bracketed chord markers. Other directives produce
diagnostics and make the `cpl` command fail. No conformance status table exists until it can be
generated from the reference implementation as required by `PROPOSAL.md`.

Next: cover the remaining directive and environment grammar, transpose and
chord definitions, then compare against the official ChordPro parser.
