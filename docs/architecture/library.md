# Library, sidecar and search

## Library (`cpl-app/src/library.rs`)

The library is every ChordPro file (`SONG_EXTENSIONS`, any case) under the
library folders, recursively, skipping hidden entries, ordered by path. The
macOS host's library folders are each folder named on the command line and
the folder of each song file named on it; launched with no arguments, the
viewer opens on search over the current folder, and closing search before
a song is opened quits. `Library::scan` runs on the page
worker when search opens, and again every `search_rescan_seconds` while
search stays open; a rescan that changes nothing redraws nothing, and one
that does keeps the selection on the same song, else the same file
(`Search::rescanned`). It
reuses the previous scan's entry for any file whose size and mtime are
unchanged. An unreadable file is skipped rather
than hiding the rest.

An `Entry` holds what search shows and matches:
- title (the file stem when there is no `{title}`), artist, key, capo,
  tempo and year;
- file-authored tags from `{tag}` and `{meta: tags …}`, which are shown and
  filtered on but never written back;
- the lower-cased lyrics, outside tablature;
- which song in its file (`part`): a file of several songs
  (`{new_song}`) gives one entry per song, and a later song without a
  title is named by its file and place, `Medley (2)`;
- the content **fingerprint**: the SHA-256 (`sha256.rs`, hand-written and
  checked against the FIPS vectors) of the song's normalised text, which
  for a file of one song is the whole file. Normalising
  drops a byte-order mark, makes line endings `\n`, and trims trailing
  whitespace on each line and at the end.

## Sidecar (`cpl-app/src/store.rs`)

Tags and ratings live in `library.toml`. By default it sits at
`~/Library/Application Support/chordpro-live/library.toml`; `--store PATH`
or `$CPL_LIBRARY_STORE` moves it. Each song is one `[song.<fingerprint>]`
table:
- `title`, a courtesy for a reader;
- `rating`, 1–5;
- `tags`, a string array;
- `opened`, when the viewer last showed the song, as a TOML UTC date-time
  (`2026-10-29T21:04:00Z`);
- `transpose`, the live transpose last set, −11 to 11 semitones; absent
  when the song is played as written.

The viewer's worker notes `opened` once per song change, after that page has
gone to the host, for songs that came from a file; `chordpro-live render` and the
built-in samples note nothing. It writes `transpose` whenever `[` `]` or `0`
change a file's song, and when a setlist is made (at launch, or by opening
a search row) each song takes its remembered transpose before anything is
measured, so a song opens in the key it was last played in. The worker
keeps each setlist song's path, fingerprint and title from when it read
the file, so neither write reads a song again. `chordpro-live render` restores a
remembered transpose but never writes one.

Keys this build does not know are kept verbatim and written back. A record
with nothing left is dropped. Saving writes a temporary file and renames
it over the old one. A file that does not parse makes the store read-only
for the session, and search says so: it is never overwritten.

## Search (`cpl-app/src/search.rs`)

`Search` holds the query, the tag and minimum-rating filters, the sort
order, the cursor and the matched rows. `input` takes a decoded
`SearchInput` and returns a `SearchAction` for the host:
- typing filters, with the best match selected;
- `+tag␣` becomes a tag filter pill and `*3` a three-stars-and-up pill;
  Backspace on an empty query removes the newest pill;
- `l:` adds lyrics to the match;
- digits 0–5 rate the selected song (`Rate`);
- `#name⏎` toggles a sidecar tag on it (`Tag`);
- Tab cycles the sort order (title, artist, rating, recent: newest
  `opened` first, never-shown songs last by title) when there is no query;
- Enter opens (`Open`: every row, in order, and the cursor's position);
- Escape cancels tag entry or closes search (`Close`);
- the query edits as a Mac text field, at a caret: ← → (Ctrl-B F), ⌘← ⌘→
  (Ctrl-A E), ⌥← ⌥→ by word, ⌥⌫ (Ctrl-W) a word back, ⌘⌫ (Ctrl-U) to the
  start, Ctrl-K to the end, ⌦ (Ctrl-D) forward; Ctrl-N P and ⌘↑ ⌘↓ move
  the selection. `insert_text` pastes as plain text, so a pasted digit
  never rates a song. The host sends ⌘V ⌘C ⌘X to the worker, which runs
  `pbpaste` or `pbcopy` there, never on the window thread; copy and cut
  take the whole query.

Matching is chordpro-tui's finder (`internal/tui/picker.go`), ported with
its tests: a case-insensitive subsequence over title and artist, scored for
word starts and runs, with an artist hit two points under a title hit. A
lyric hit, when asked for, ranks under both.

`frame` composes the Figma mock's Search screen in points from the
`search_*` config keys:
- a query line: a soft magnifying glass (drawn as a ring and a handle of
  discs) or `#` while a tag is typed, the query and its caret, the hint
  under the query, then a rule;
- `FILTERS` and its pills: filled for active filters (paper text), outlined
  for `+ tag` and the sort;
- a results box with a bold header row (the rating column is `RATING`) and
  one row per match, with matched
  letters in bold and the selected row in the wash ink;
- a footer, on the page status strip's baseline, with counts, folders or
  the last action's notice, and the keys as keycaps: a washed, outlined
  chip per key in strong ink with its action beside it in soft ink.

Metadata columns are as wide as their widest value or header plus
`search_column_gap`. Title and artist share the rest, 58:42. While they
would get less than `search_min_title` each, columns drop in
chordpro-tui's order: year, tempo, capo, tags, key, rating.

## Host

The window thread sends the worker `ToWorker` messages. Page requests
coalesce to the newest; `OpenSearch` and every `Search` key are applied in
order, so typing never loses a key. While search is up the worker composes
its frame whole for each request, and `ReadyPage::searching` routes keys.

An `Open` reads and parses the rows' files and makes them a new `Setlist`
at the chosen song. It bumps the setlist id: the host adopts the worker's
song, count and position, and `n`/`p` then step through what was filtered,
in its order. `/` or `o` opens search over the page. `--search QUERY` opens
the viewer, or `chordpro-live render`, in search with that query typed. In the
viewer, `--open N` then sends the worker Home, N−1 Downs and Enter, the
same inputs as the keys, and `--next K` steps K songs once the opened
setlist is adopted, through the same path as `n`/`p`. `chordpro-live render
<folder>` renders a folder's first song or, with `--search`, its search
screen.
