# History

This is a record of implemented work and observed checks. `TASKS.md` holds
open work; `PROPOSAL.md` holds design decisions. Dates are local to the project
session. A passing headless test does not stand in for a visual window check.

## 2026-10-20

- **Tablature hidden from the macOS performance page.** `cpl-app` now measures
  and fits a display song without `Tab` sections while retaining the complete
  parsed song. Headless tests cover fit and scroll output. This responds to a
  user screenshot where a long tab dominated the page; the corrected window
  has not yet been captured with that source song.
- `22642aa` — added point-size candidates from the 17 pt floor to a
  configurable 28 pt maximum, measured at their actual drawn sizes. Workspace
  tests and strict Clippy passed. Visual verification was still outstanding.
- `c3906be` — reused the prepared scroll page and cached CoreText coverage by
  text, face, weight, size, and display scale. Scrolling still composites and
  uploads the visible page texture.
- `b8bd4bf` — included header width in the fit decision, so an overflowing
  title cannot be accepted as a fitting page.

Earlier implementation and design history is available in Git. New entries
should state what shipped, what was checked, and what remains unverified.
