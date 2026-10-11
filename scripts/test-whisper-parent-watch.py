#!/usr/bin/env python3
"""Verify the macOS Whisper sidecar exits after its owning process disappears."""

import argparse
import os
import pathlib
import subprocess
import sys
import time


PARENT = r"""
import os, socket, subprocess, sys, time, urllib.request
server, model = sys.argv[1:3]
with socket.socket() as sock:
    sock.bind(('127.0.0.1', 0))
    port = sock.getsockname()[1]
child = subprocess.Popen(
    [server, '-m', model, '--host', '127.0.0.1', '--port', str(port), '--request-path', '/qa', '-l', 'auto'],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
for _ in range(300):
    if child.poll() is not None:
        raise RuntimeError('Whisper sidecar exited before loading')
    try:
        urllib.request.urlopen(f'http://127.0.0.1:{port}/qa/health', timeout=1).read()
        break
    except Exception:
        time.sleep(0.1)
else:
    raise RuntimeError('Whisper sidecar did not become ready')
print(child.pid, flush=True)
os._exit(0)
"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True, type=pathlib.Path)
    parser.add_argument("--server", type=pathlib.Path)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("this check requires macOS")
    triple = next(
        line.split(": ", 1)[1]
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    server = args.server or (
        pathlib.Path(__file__).resolve().parents[1] / "src-tauri/binaries" / f"whisper-server-{triple}"
    )
    if not server.is_file() or not args.model.is_file():
        parser.error("Whisper sidecar or model is missing")

    parent = subprocess.Popen(
        [sys.executable, "-c", PARENT, str(server), str(args.model.resolve())],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        output, error = parent.communicate(timeout=40)
    except subprocess.TimeoutExpired:
        parent.kill()
        parent.communicate()
        raise RuntimeError("watch parent timed out")
    if parent.returncode != 0:
        raise RuntimeError(f"watch parent failed: {error[:300]}")
    pid = int(output.strip())
    for _ in range(50):
        state = subprocess.run(
            ["ps", "-p", str(pid), "-o", "stat="], capture_output=True, text=True
        ).stdout.strip()
        if not state or state.startswith("Z"):
            print("parent_exit_reaps_sidecar=true")
            return
        time.sleep(0.1)
    try:
        os.kill(pid, 9)
    except ProcessLookupError:
        pass
    raise RuntimeError("Whisper sidecar survived parent exit")


if __name__ == "__main__":
    main()
