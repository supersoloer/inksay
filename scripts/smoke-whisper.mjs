import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .match(/^host: (.+)$/m)?.[1];
if (!triple) throw new Error("Unable to determine Rust host target");

const suffix = process.platform === "win32" ? ".exe" : "";
const sidecar = resolve(import.meta.dirname, `../src-tauri/binaries/whisper-server-${triple}${suffix}`);
if (!existsSync(sidecar)) throw new Error(`Whisper sidecar is missing: ${sidecar}`);
execFileSync(sidecar, ["--help"], { stdio: "pipe", timeout: 10_000 });
process.stdout.write(`Whisper sidecar starts: ${sidecar}\n`);
