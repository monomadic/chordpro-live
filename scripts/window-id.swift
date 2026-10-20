// Prints the CGWindowID of every on-screen chordpro-live window (the bare
// `cpl` binary), one per line — what `screencapture -l` wants for a targeted
// capture, so a visual check never injects keystrokes:
//
//   screencapture -x -l "$(swift scripts/window-id.swift | head -1)" shot.png
import CoreGraphics

let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]]
for w in list where ((w["kCGWindowOwnerName"] as? String) ?? "").lowercased() == "cpl" {
    print(w["kCGWindowNumber"] as! Int)
}
