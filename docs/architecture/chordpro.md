# ChordPro parsing

`cpl-song` owns source parsing and the platform-independent `Song` model,
written against the working spec in `~/src/chordpro-library`. Chord markers
split a lyric line into segments so each chord belongs to the lyric text
immediately after it. Original spaces in each segment are kept.

`parse` reads one song. A file is read with `split`, which cuts it at each
`{new_song}` (`{ns}`) outside tablature and notation into `Part`s (each
song's text and the lines before it), or with `parse_all`, which parses
every part and counts diagnostic lines from the file's top. A file of one
song is one part whose text is the whole file, so its fingerprint is the
file's; a part with nothing but blank lines and remarks is no song.

The parser is **not yet ChordPro 6 conformant**. What it handles:

- Directives with a colon and/or whitespace before the argument, names
  case-insensitive, long and short forms (`t`, `st`, `c`, `ci`, `cb`, `sov`,
  `eov`, `soc`, `eoc`, `sob`, `eob`, `sot`, `eot`, `sog`, `eog`, `ns`).
- Selectors: `{name-selector: …}` applies when the selector is the
  instrument (`INSTRUMENT`, "guitar", the table's diagrams) or a metadata
  item set to something other than empty, `0`, `false` or `null`; a
  trailing `!` negates. A `start_of_…` that does not apply skips everything
  to its `end_of_…`, which carries no selector. There is no user name.
- `%{…}` substitution (`substitute.rs`) in metadata values, comments,
  labels, lyrics and annotations, from the metadata as it stands at that
  line: `%{name}`, `%{name.2}`, `%{name.-1}`, `%{name|yes|no}`,
  `%{name=value|yes|no}`, and `%{}` for the controlling value inside a
  branch. Several values join with `"; "`; an unknown item is empty; a
  malformed one (`%{ name }`, no closing brace) and anything escaped
  outside a branch stay as written. Each rule is tested against the
  reference's output for the same text.
- The standard metadata items and `{meta: name value}`.
- Verse, chorus, bridge, tab and textblock environments, labelled by a bare
  argument or a quoted `label="…"` attribute. Tablature is verbatim, with
  tab characters expanded to eight-column stops. A text block's lines parse
  as a verse's do.
- Grid environments, labelled only by `label="…"` (a bare argument is the
  shape). Each row is its cells, spacing aside: the margin before the
  first bar (always a cell, perhaps empty), then each bar, `.`, `/`, `%`
  and chord, then the comment after the last bar if any. Margin and comment
  are kept as written; chords between the bars transpose, and `C~G` shares
  a cell. `Line::cells` gives them in order.
- The delegated `abc`, `ly` and `svg` environments are kept verbatim as
  `SectionKind::Embedded` and diagnosed as not drawn yet.
- `[*text]` is an annotation: `Segment::annotation`, shown where a chord
  would be, never transposed or given a diagram.
- `{chorus}` and `{chorus: Label}` recall the latest chorus: the section's
  `recall_of` names the chorus as written, and its lines are a copy.
- Blank lines end a stanza that no environment encloses; lines before the
  first environment are such a stanza.
- `{comment}`, `{comment_italic}`, `{comment_box}` and `{highlight}` are
  visible `LineKind::Comment` lines; `#` remarks are dropped.
- Formatting directives (the `*font`, `*size`, `*colour` families and their
  short forms, `titles`, `diagrams`, `grid`, `no_grid`, `pagetype`) and `x_*`
  private directives are accepted and ignored by design.
- `{define}` with `base-fret` (or `base_fret`), `frets` (`x` or `N` mutes)
  and `fingers`; a barre is inferred where one finger holds two or more
  strings at one fret. `Song::defines` keeps them by name, and
  `Song::shape` prefers them to the built-in table.
- `{transpose: n}`, `+n`, `-n`, and `nf` or `ns` to force flats or
  sharps; empty cancels. Chords after it are shifted as parsed, tablature
  excepted, and `key_actual` becomes the key the player reads; `{key}`
  stays as written. Without a suffix the shifted root takes the fixed
  spelling (E♭ and B♭ flat; C♯, F♯ and G♯ sharp), not the spec's
  sharps-up, flats-down, as PROPOSAL.md decides.
- UTF-8 BOM and CRLF.

## Transpose

`transpose::chord` shifts a chord name, its slash bass too, ported from
chordpro-tui's `transpose.go` with its tests; a name that does not start
with a note is left alone, and a shift of zero keeps the author's spelling.
`Song::transposed(n)` is the viewer's live transpose: a copy with every
chord and `key`/`key_actual` shifted, tablature verbatim, and the offset
added to `Song::transpose` for the status strip. `{define}` voicings keep
their names, so a shifted chord takes the built-in shape unless the song
defines that chord too.

Diagnosed, which makes the `cpl` command fail: mismatched, orphaned or
unclosed environments, recall before any chorus, empty comments, layout
hints (`new_page`, `column_break`, `columns`) until the fit engine honours
them, embedded notation until it is drawn, a `{new_song}` given to `parse`
rather than `parse_all`, and every other directive. Not yet parsed:
chords inside a comment (kept as text), markup, keyboard `{define}`s and
their `copy`/`display` modifiers, `{chord}`, and the read-only `_key`.
Substitution happens as the song is parsed, so a `%{key_actual}` in the
text does not follow the viewer's live transpose. Comments render in the
soft ink at lyric size until the Figma mock defines a comment style.

## Conformance

`cpl conform` parses the same ChordPro with the reference implementation
(the `chordpro` program, `cpan App::Music::ChordPro`; found as
`$CPL_CHORDPRO`, on the PATH, or in `~/perl5`) and with `cpl-song`, and
compares what each understood: how many songs, the standard metadata, the
section structure (kind, label and line count of each verse, chorus,
recalled chorus, tab, grid, text block, notation or bare stanza), the chords
in order, and the lyric, comment, tab and grid text. Notation counts as one
item, as the reference renders it to one image; one the reference lifts to
the top of the page (`spread=`) is compared where the file wrote it. The reference speaks through
`scripts/conform/ChordPro/Output/CplJSON.pm`, an output backend loaded with
`--generate=CplJSON` that writes its parse as JSON; it runs with
`--no-default-configs`, so no user config changes the answer.

The table below is generated by `cpl conform --table
docs/architecture/chordpro.md` and checked with `--check`; never edit it by
hand. Each construct row is a probe song written in `src/conform.rs`. "cpl-song
says" is the parser's own diagnostic for it: "no diagnostic" means it
parsed without complaint, not that it renders as the spec intends (markup,
for one, is kept as literal text). "Same parse" compares only what the
model above covers, so chord diagrams (`{define}`, `{chord}`), images and
layout hints agree trivially. The file rows are `testdata/` and the
reference's own examples.

<!-- conform:begin (generated by `cpl conform --table`; do not edit) -->
Compared with the reference implementation (This is ChordPro core 6.101.0): 59 of 60 constructs and 7 of 7 files parse alike.

| Construct | cpl-song says | Same parse as the reference |
|---|---|---|
| `{title}` | no diagnostic | yes |
| `{subtitle}` | no diagnostic | yes |
| `{artist}` | no diagnostic | yes |
| `{sortartist}` | no diagnostic | yes |
| `{composer}` | no diagnostic | yes |
| `{lyricist}` | no diagnostic | yes |
| `{arranger}` | no diagnostic | yes |
| `{copyright}` | no diagnostic | yes |
| `{album}` | no diagnostic | yes |
| `{year}` | no diagnostic | yes |
| `{key}` | no diagnostic | yes |
| `{time}` | no diagnostic | yes |
| `{tempo}` | no diagnostic | yes |
| `{duration}` | no diagnostic | yes |
| `{capo}` | no diagnostic | yes |
| `{sorttitle}` | no diagnostic | yes |
| `{tag}` | no diagnostic | yes |
| `{meta}` | no diagnostic | yes |
| `{comment}` | no diagnostic | yes |
| `{comment} with chords` | no diagnostic | no (lyrics): item 1: cpl-song has "comment: [C]Capo [G]up", the reference has "comment: Capo up" |
| `{comment_italic}` | no diagnostic | yes |
| `{comment_box}` | no diagnostic | yes |
| `{highlight}` | no diagnostic | yes |
| `{start_of_verse}` | no diagnostic | yes |
| `{start_of_chorus}` | no diagnostic | yes |
| `{start_of_bridge}` | no diagnostic | yes |
| `{start_of_tab}` | no diagnostic | yes |
| `{start_of_grid}` | no diagnostic | yes |
| `grid margins, comments and shared cells` | no diagnostic | yes |
| `{chorus}` | no diagnostic | yes |
| `{define}` | no diagnostic | yes |
| `{chord}` | diagnosed: unsupported directive: chord | yes |
| `{transpose}` | no diagnostic | yes |
| `{start_of_abc}` | diagnosed: notation not drawn yet: abc | yes |
| `{start_of_ly}` | diagnosed: notation not drawn yet: ly | yes |
| `{start_of_svg}` | diagnosed: notation not drawn yet: svg | yes |
| `{start_of_textblock}` | no diagnostic | yes |
| `{new_song}` | no diagnostic | yes |
| `{ns}` | no diagnostic | yes |
| `{new_page}` | diagnosed: layout hint not honoured yet: new_page | yes |
| `{new_physical_page}` | diagnosed: layout hint not honoured yet: new_physical_page | yes |
| `{column_break}` | diagnosed: layout hint not honoured yet: column_break | yes |
| `{columns}` | diagnosed: layout hint not honoured yet: columns | yes |
| `{pagetype}` | accepted, ignored | yes |
| `{titles}` | accepted, ignored | yes |
| `{diagrams}` | accepted, ignored | yes |
| `{grid}` | accepted, ignored | yes |
| `{textfont} and the font, size, colour families` | accepted, ignored | yes |
| `{image}` | diagnosed: unsupported directive: image | yes |
| `{x_…} private directives` | accepted, ignored | yes |
| `[*annotation]` | no diagnostic | yes |
| `chord-only line` | no diagnostic | yes |
| `mid-word chord` | no diagnostic | yes |
| `<b> markup` | no diagnostic | yes |
| `%{…} substitution` | no diagnostic | yes |
| `%{…\|…\|…} conditionals` | no diagnostic | yes |
| `{comment-guitar} selector` | no diagnostic | yes |
| `negated selector` | no diagnostic | yes |
| `metadata item as selector` | no diagnostic | yes |
| `selector on an environment` | no diagnostic | yes |

| File | Same parse as the reference |
|---|---|
| crowded_chords.cho | yes |
| greensleeves.cho | yes |
| harbour_lights.cho | yes |
| house_of_the_rising_sun.cho | yes |
| scarborough_fair.cho | yes |
| mollymalone.cho | yes |
| swinglow.cho | yes |
<!-- conform:end -->

## Chord shapes

`chords::lookup` resolves a chord name to a guitar fingering from
`data/guitar.tsv`: 828 chords, the first position of each from
tombatossals/chords-db (MIT), generated by `scripts/chords-from-json.py`
from chordpro-tui's copy of the JSON. A name splits into a root (with `#`
or `b`) and a suffix; the suffix is tried as written, then through a small
alias table (`m` → `minor`, `+` → `aug`, …), and a slash chord whose bass
the table lacks falls back to its base chord. Ported with its tests from
chordpro-tui/internal/chords/db.go. `Song::chord_names` lists the chords a
song uses, outside tablature and annotations, in first-use order; a grid
cell's `C~G` gives both.
