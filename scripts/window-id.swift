// Prints the CGWindowID of every on-screen chordpro-live window (the bare
// `chordpro-live` binary, or ChordProLive.app, which owns its windows as
// "ChordPro Live"), one per line — what `screencapture -l` wants for a targeted
// capture, so a visual check never injects keystrokes:
//
//   screencapture -x -l "$(swift scripts/window-id.swift | head -1)" shot.png
import CoreGraphics

let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]]
let owners: Set<String> = ["chordpro-live", "chordpro live"]
for w in list where owners.contains(((w["kCGWindowOwnerName"] as? String) ?? "").lowercased()) {
    print(w["kCGWindowNumber"] as! Int)
}
