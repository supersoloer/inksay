# Silero VAD

Bundled model: `ggml-silero-v6.2.0.bin` (885098 bytes).
Source: https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin
SHA-256: `2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987`

Converted model supplied by ggml-org for whisper.cpp. Original Silero VAD: https://github.com/snakers4/silero-vad
License: MIT; see SILERO-LICENSE. No runtime model download is needed. Build tooling verifies the bundled checksum.

# Recognition configuration test

`asr-test.wav` is the first four seconds of the JFK inaugural-address sample in the pinned whisper.cpp repository (`samples/jfk.wav`, commit f24588a272ae8e23280d9c220536437164e6ed28). The source speech is a US federal government work in the public domain. Only an explicit configuration-test action sends this sample to the user's configured service; no personal history recording is used.

# Cloud audio compression

FLAC encoding uses flacenc 0.5.1 (https://github.com/yotarok/flacenc-rs/), Apache-2.0.
The encoder is built into the app; no external codec installation is required. See FLACENC-LICENSE.
