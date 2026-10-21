#!/usr/bin/env python3
"""Capture review candidates, never approve or overwrite a baseline."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def cases():
    probes = [
        ("scarborough", "testdata/scarborough_fair.cho", "420x720", 0),
        ("scroll-top", "page-scroll", "360x420", 0),
        ("scroll-middle", "page-scroll", "360x420", 120),
        ("scroll-end", "page-scroll", "360x420", 100000),
        ("alignment", "tests/visual/wrapping.cho", "420x720", 0),
        ("width-needed", "tests/visual/unbreakable.cho", "360x420", 0),
        ("type-ceiling", "tests/visual/type-ceiling.cho", "980x720", 0),
        ("inline", "page-inline", "360x420", 0),
    ]
    for name, source, size, scroll in probes:
        for dark in (False, True):
            # CLI only supports a dark built-in default page. Use its identical
            # file fixture to reach the scroll sample in dark polarity.
            target = "tests/visual/page-scroll.cho" if dark and source == "page-scroll" else source
            args = ["--view"]
            if dark:
                args += ["page-dark"]
                if source == "page-inline":
                    # This built-in has a distinct inline-mode flag; don't
                    # pretend a file or the default dark page exercises it.
                    continue
            args += [target, "--size", size, "--scroll", str(scroll)]
            yield {"name": name + ("-dark" if dark else "-light"),
                   "args": args, "size": size}


def run(command, **kwargs):
    return subprocess.run(command, check=True, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="print matrix without building or opening windows")
    parser.add_argument("--output", type=Path, help="new directory for candidates, logs and manifest")
    parser.add_argument("--case", action="append", help="capture only this exact matrix name (repeatable)")
    opts = parser.parse_args()
    matrix = list(cases())
    if opts.case:
        unknown = set(opts.case) - {case["name"] for case in matrix}
        if unknown:
            parser.error("unknown cases: " + ", ".join(sorted(unknown)))
        matrix = [case for case in matrix if case["name"] in opts.case]
    if opts.list:
        print(json.dumps(matrix, indent=2))
        return 0
    if not opts.output:
        parser.error("--output is required (must not already exist)")
    if platform.system() != "Darwin":
        parser.error("window captures require macOS with an awake graphical session")
    out = opts.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    manifest = {"status": "incomplete", "review": "pending human review",
                "revision": run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True).stdout.strip(),
                "working_tree": run(["git", "status", "--short"], cwd=ROOT, capture_output=True).stdout,
                "system": platform.platform(), "cases": []}
    def save():
        (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    save()
    try:
        run(["cargo", "build", "--locked"], cwd=ROOT)
        metadata = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, capture_output=True).stdout)
        binary = Path(metadata["target_directory"]) / "debug/cpl"
        manifest["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        manifest["inputs_sha256"] = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for directory in [ROOT / "testdata", ROOT / "tests/visual"]
            for p in sorted(directory.rglob("*.cho"))}
        manifest["config_sha256"] = hashlib.sha256((ROOT / "config/default.toml").read_bytes()).hexdigest()
        # Compile once, using a cache writable in restricted environments.
        run(["swiftc", "-module-cache-path", str(out / "swift-cache"),
             str(ROOT / "scripts/capture-window.swift"), "-o", str(out / "capture-window")])
        run([str(out / "capture-window"), "--preflight"])
        for case in matrix:
            record = dict(case, status="incomplete")
            manifest["cases"].append(record)
            save()
            log_path = out / (case["name"] + ".log")
            with log_path.open("w") as log:
                proc = subprocess.Popen([str(binary)] + case["args"], cwd=ROOT,
                    env=dict(os.environ, CPL_TIMING="1"), stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + 30
                    while "slab " not in log_path.read_text():
                        if proc.poll() is not None:
                            raise RuntimeError(f"{case['name']}: viewer exited; inspect {log_path}")
                        if time.monotonic() > deadline:
                            raise RuntimeError(f"{case['name']}: no composed page within 30 seconds")
                        time.sleep(0.1)
                    # Worker readiness precedes presentation; allow the event
                    # loop to upload and present. Human review still rejects a
                    # blank, stale or partially presented candidate.
                    time.sleep(1.5)
                    width, height = case["size"].split("x")
                    record["window"] = json.loads(run([str(out / "capture-window"), str(proc.pid), width, height], capture_output=True).stdout)
                    png = out / (case["name"] + ".png")
                    run(["/usr/sbin/screencapture", "-x", "-o", "-l",
                         str(record["window"]["window_id"]), str(png)])
                    if not png.exists() or png.stat().st_size == 0:
                        raise RuntimeError("No image produced")
                    record["image_sha256"] = hashlib.sha256(png.read_bytes()).hexdigest()
                    record["image_info"] = run(["sips", "-g", "pixelWidth", "-g", "pixelHeight", str(png)], capture_output=True).stdout
                    record["status"] = "captured; review pending"
                finally:
                    if proc.poll() is None:
                        proc.terminate()
                        try:
                            proc.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            proc.kill()
                            proc.wait()
                save()
        manifest["status"] = "captured; review pending"
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        manifest["error"] = str(error)
        print(str(error), file=sys.stderr)
        return 1
    finally:
        save()
    return 0


if __name__ == "__main__":
    sys.exit(main())
