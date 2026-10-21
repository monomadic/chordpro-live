// Target one process and reject unavailable or differently sized windows.
import AppKit
import CoreGraphics
import Foundation

func fail(_ message: String) -> Never {
    fputs(message + "\n", stderr)
    exit(1)
}
guard CGPreflightScreenCaptureAccess() else {
    fail("Screen Recording permission is required for the invoking terminal/app")
}
guard CGDisplayIsAsleep(CGMainDisplayID()) == 0 else { fail("Display is asleep") }
if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "--preflight" {
    print("capture preflight passed")
    exit(0)
}
guard CommandLine.arguments.count == 4,
      let pid = Int(CommandLine.arguments[1]),
      let width = Double(CommandLine.arguments[2]),
      let height = Double(CommandLine.arguments[3]) else {
    fail("usage: capture-window.swift PID WIDTH HEIGHT | --preflight")
}
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let matches = windows.filter {
    ($0[kCGWindowOwnerPID as String] as? Int) == pid &&
    ($0[kCGWindowLayer as String] as? Int) == 0
}
guard matches.count == 1, let window = matches.first,
      let bounds = window[kCGWindowBounds as String] as? [String: Double],
      let w = bounds["Width"], let h = bounds["Height"],
      let id = window[kCGWindowNumber as String] as? Int else {
    fail("Expected exactly one on-screen window for process \(pid)")
}
guard abs(w - width) < 1 && abs(h - height) < 1 else {
    fail("Window is \(w)x\(h) logical points; expected \(width)x\(height)")
}
let result: [String: Any] = ["window_id": id, "bounds": bounds,
    "displays": NSScreen.screens.map { ["scale": $0.backingScaleFactor,
        "width": $0.frame.width, "height": $0.frame.height] }]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
