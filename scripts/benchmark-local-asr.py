#!/usr/bin/env python3
"""Compare local whisper.cpp output with saved transcripts without printing either."""

import argparse
import json
import pathlib
import socket
import statistics
import subprocess
import tempfile
import time
import urllib.request


TAURI = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_DATA = pathlib.Path.home() / "Library/Application Support/com.supersoloer.inksay.macos"
DEFAULT_SERVER = TAURI / "src-tauri/target/release/bundle/macos/InkSay.app/Contents/MacOS/whisper-server"


def character_error_rate(reference: str, recognized: str) -> float:
    reference = "".join(ch for ch in reference.casefold() if ch.isalnum())
    recognized = "".join(ch for ch in recognized.casefold() if ch.isalnum())
    previous = list(range(len(recognized) + 1))
    for index, character in enumerate(reference, 1):
        current = [index]
        for offset, candidate in enumerate(recognized, 1):
            current.append(min(
                current[-1] + 1,
                previous[offset] + 1,
                previous[offset - 1] + (character != candidate),
            ))
        previous = current
    return previous[-1] / max(1, len(reference))


def simplified(texts: list[str]) -> list[str]:
    script = (
        'import OpenCC from "opencc-js/t2cn"; '
        'let input=""; for await (const chunk of process.stdin) input+=chunk; '
        'const convert=OpenCC.Converter({from:"tw",to:"cn"}); '
        'process.stdout.write(JSON.stringify(JSON.parse(input).map(convert)));'
    )
    result = subprocess.run(
        ["node", "--input-type=module", "-e", script],
        input=json.dumps(texts), capture_output=True, text=True, check=True, cwd=TAURI,
    )
    return json.loads(result.stdout)


def wait_for_server(child: subprocess.Popen, address: str) -> float:
    started = time.monotonic()
    for _ in range(600):
        if child.poll() is not None:
            raise RuntimeError("local ASR server exited during model loading")
        try:
            urllib.request.urlopen(address + "/health", timeout=1).read()
            return time.monotonic() - started
        except Exception:
            time.sleep(0.1)
    raise RuntimeError("local ASR model loading timed out")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True, type=pathlib.Path)
    parser.add_argument("--app-data", type=pathlib.Path, default=DEFAULT_DATA)
    parser.add_argument("--server", type=pathlib.Path, default=DEFAULT_SERVER)
    args = parser.parse_args()
    if not args.model.is_file() or not args.server.is_file():
        parser.error("model or server does not exist")

    document = json.loads((args.app_data / "history.json").read_text())
    if document["schemaVersion"] != 1:
        raise ValueError("Unsupported history schema")
    history = document["data"]
    eligible = [
        row for row in history
        if row.get("transcript")
        and row.get("audioFileName")
        and pathlib.Path(row["audioFileName"]).name == row["audioFileName"]
        and (args.app_data / "recordings" / row["audioFileName"]).is_file()
        and float(row.get("duration", 0)) >= 1.5
    ]
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    address = f"http://127.0.0.1:{port}/qa"
    child = subprocess.Popen(
        [str(args.server), "-m", str(args.model), "--host", "127.0.0.1", "--port", str(port), "--request-path", "/qa", "-l", "auto"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    compared: list[tuple[str, str, float, float, str]] = []
    conversion_errors = 0
    inference_errors = 0
    try:
        cold_load = wait_for_server(child, address)
        with tempfile.TemporaryDirectory() as temporary:
            for index, row in enumerate(eligible):
                source = args.app_data / "recordings" / row["audioFileName"]
                wav = pathlib.Path(temporary) / f"sample-{index}.wav"
                converted = subprocess.run(
                    ["ffmpeg", "-nostdin", "-loglevel", "error", "-i", str(source), "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(wav)],
                    capture_output=True,
                )
                if converted.returncode:
                    conversion_errors += 1
                    continue
                started = time.monotonic()
                response = subprocess.run(
                    ["curl", "--silent", "--show-error", "--fail", "-F", f"file=@{wav}", "-F", "response_format=json", "-F", "language=zh", "-F", "temperature=0.0", address + "/inference"],
                    capture_output=True, text=True, timeout=120,
                )
                if response.returncode:
                    inference_errors += 1
                    continue
                try:
                    recognized = json.loads(response.stdout)["text"]
                except (KeyError, ValueError):
                    inference_errors += 1
                    continue
                compared.append((
                    row["transcript"], recognized, time.monotonic() - started,
                    float(row.get("transcriptionDuration") or 0),
                    str(row.get("backend") or "unknown"),
                ))
        normalized = simplified([text for reference, recognized, *_ in compared for text in (reference, recognized)])
        scored = [
            (character_error_rate(reference, recognized), row[4])
            for row, reference, recognized in zip(compared, normalized[::2], normalized[1::2])
        ]
        scores = sorted(score for score, _ in scored)
        if not scores:
            raise RuntimeError("no recordings were compared")
        print("model", args.model.name)
        print("eligible", len(eligible), "compared", len(scores), "conversion_errors", conversion_errors, "inference_errors", inference_errors)
        print("CER_mean", round(statistics.mean(scores), 3), "CER_median", round(statistics.median(scores), 3), "CER_p90", round(scores[min(len(scores) - 1, int(len(scores) * 0.9))], 3))
        print("exact", sum(score == 0 for score in scores), "above_0.3", sum(score > 0.3 for score in scores))
        print("cold_load_seconds", round(cold_load, 3), "ASR_mean_seconds", round(statistics.mean(row[2] for row in compared), 3), "Swift_saved_ASR_mean_seconds", round(statistics.mean(row[3] for row in compared), 3))
        for source in ("Large v3 Turbo", "本机 Whisper · Small", "本机 Whisper · small", "云端转写 · Systran/faster-whisper-small"):
            subset = [score for score, backend in scored if source in backend]
            if subset:
                print("reference_backend", source, "count", len(subset), "CER_mean", round(statistics.mean(subset), 3), "exact", sum(score == 0 for score in subset))
    finally:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


if __name__ == "__main__":
    main()
