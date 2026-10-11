#!/usr/bin/env python3
"""Test the bundled local VAD gate with synthetic noise and supplied speech WAVs.
Never print transcripts or upload audio. Speech inputs must be 16 kHz mono PCM16.
"""
import argparse
import array
import json
import math
import pathlib
import random
import socket
import statistics
import subprocess
import time
import urllib.request
import uuid
import wave


def wav_bytes(samples):
    import io
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as writer:
        writer.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        writer.writeframes(array.array("h", (int(max(-32768, min(32767, x))) for x in samples)).tobytes())
    return buffer.getvalue()


def detect(address, audio):
    boundary = uuid.uuid4().hex
    body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="sample.wav"\r\n\r\n'.encode()
            + audio + f'\r\n--{boundary}--\r\n'.encode())
    start = time.monotonic()
    request = urllib.request.Request(address + "/vad", body, {"Content-Type": f"multipart/form-data; boundary={boundary}"})
    result = json.loads(urllib.request.urlopen(request, timeout=30).read())
    return result, round((time.monotonic() - start) * 1000, 2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=pathlib.Path)
    parser.add_argument("--model", type=pathlib.Path)
    parser.add_argument("--speech", type=pathlib.Path, action="append", required=True)
    parser.add_argument("--non-speech", type=pathlib.Path, action="append", default=[])
    args = parser.parse_args()
    if args.server is None:
        import platform
        host = next(line[6:] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
        suffix = ".exe" if platform.system() == "Windows" else ""
        args.server = pathlib.Path(__file__).resolve().parents[1] / f"src-tauri/binaries/whisper-server-{host}{suffix}"
    vad = pathlib.Path(__file__).resolve().parents[1] / "src-tauri/resources/ggml-silero-v6.2.0.bin"
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    address = f"http://127.0.0.1:{port}/vad-check"
    model_args = ["-m", str(args.model.resolve())] if args.model else ["--vad-only"]
    child = subprocess.Popen([str(args.server.resolve()), *model_args, "--vad-model", str(vad),
                              "--host", "127.0.0.1", "--port", str(port), "--request-path", "/vad-check"],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    rows = []
    try:
        for _ in range(300):
            if child.poll() is not None:
                raise RuntimeError("sidecar exited during startup")
            try:
                urllib.request.urlopen(address + "/health", timeout=1).close()
                break
            except OSError:
                time.sleep(0.1)
        else:
            raise RuntimeError("sidecar startup timed out")
        random.seed(731)
        cases = []
        for index, path in enumerate(args.speech):
            with wave.open(str(path), "rb") as reader:
                assert (reader.getnchannels(), reader.getsampwidth(), reader.getframerate()) == (1, 2, 16000)
                samples = array.array("h", reader.readframes(reader.getnframes()))
            for gain in (1, 0.1, 0.03):
                cases.append((f"speech-{index}-gain-{gain}", wav_bytes(x * gain for x in samples), True))
        cases.extend([
            ("silence", wav_bytes([0] * 48000), False),
            ("white-noise", wav_bytes(random.gauss(0, 350) for _ in range(48000)), False),
            ("fan-simulation", wav_bytes(500 * math.sin(2 * math.pi * 80 * i / 16000) + random.gauss(0, 40) for i in range(48000)), False),
            ("click-simulation", wav_bytes(random.gauss(0, 1000) if i % 8000 < 120 else random.gauss(0, 20) for i in range(48000)), False),
        ])
        cases.extend((f"non-speech-{i}", path.read_bytes(), False) for i, path in enumerate(args.non_speech))
        for name, audio, expected in cases:
            result, elapsed = detect(address, audio)
            assert result["speech"] == (result["segments"] > 0), name
            rows.append({"case": name, "expected_speech": expected, **result, "elapsed_ms": elapsed})
        assert urllib.request.urlopen(address + "/health", timeout=1).status == 200
        failures = [row["case"] for row in rows if row["speech"] != row["expected_speech"]]
        print(json.dumps({"cases": rows, "failures": failures, "warm_median_ms": statistics.median(row["elapsed_ms"] for row in rows[1:])}, indent=2))
        assert not failures, failures
    finally:
        child.terminate()
        child.wait(timeout=10)


if __name__ == "__main__":
    main()
