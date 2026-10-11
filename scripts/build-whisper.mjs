import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const tauri = resolve(import.meta.dirname, "../src-tauri");
const source = join(tauri, ".whisper-src");
const build = join(source, "build");
const revision = "f24588a272ae8e23280d9c220536437164e6ed28"; // whisper.cpp v1.8.5
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .match(/^host: (.+)$/m)?.[1];
if (!triple) throw new Error("Unable to determine Rust host target");

const suffix = process.platform === "win32" ? ".exe" : "";
const binaryName = `whisper-server${suffix}`;
const target = join(tauri, "binaries", `whisper-server-${triple}${suffix}`);
const stamp = `${target}.build-id`;
if (!existsSync(source)) {
  execFileSync("git", ["clone", "--depth", "1", "--branch", "v1.8.5", "https://github.com/ggml-org/whisper.cpp.git", source], { stdio: "inherit" });
}
const checkedOut = execFileSync("git", ["-C", source, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
if (checkedOut !== revision) throw new Error(`Unexpected whisper.cpp revision: ${checkedOut}`);
const vadModel = join(tauri, "resources", "ggml-silero-v6.2.0.bin");
const vadHash = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987";
if (createHash("sha256").update(readFileSync(vadModel)).digest("hex") !== vadHash) {
  throw new Error("Bundled Silero VAD model checksum mismatch");
}
const patches = [resolve(import.meta.dirname, "whisper-vad-gate.patch"), resolve(import.meta.dirname, "whisper-vad-only.patch")];
if (process.platform === "darwin") patches.push(resolve(import.meta.dirname, "whisper-parent-watch.patch"));
const patchHashes = [];
for (const patch of patches) {
  patchHashes.push(createHash("sha256").update(readFileSync(patch)).digest("hex"));
  const applied = spawnSync("git", ["-C", source, "apply", "--reverse", "--check", patch], { stdio: "ignore" }).status === 0;
  if (!applied) {
    execFileSync("git", ["-C", source, "apply", "--check", patch], { stdio: "inherit" });
    execFileSync("git", ["-C", source, "apply", patch], { stdio: "inherit" });
  }
}
const buildId = `${revision}:${patchHashes.join(":")}`;
if (!existsSync(target) || !existsSync(stamp) || readFileSync(stamp, "utf8").trim() !== buildId) {
  const options = ["-S", source, "-B", build, "-DCMAKE_BUILD_TYPE=Release", "-DBUILD_SHARED_LIBS=OFF", "-DGGML_NATIVE=OFF", "-DWHISPER_BUILD_TESTS=OFF"];
  if (process.platform === "win32") {
    options.push("-DCMAKE_POLICY_DEFAULT_CMP0091=NEW", "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded");
  }
  execFileSync("cmake", options, { stdio: "inherit" });
  execFileSync("cmake", ["--build", build, "--target", "whisper-server", "--config", "Release", "--parallel", "4"], { stdio: "inherit" });
  mkdirSync(join(tauri, "binaries"), { recursive: true });
  const candidates = [join(build, "bin", binaryName), join(build, "bin", "Release", binaryName)];
  const built = candidates.find(existsSync);
  if (!built) throw new Error(`whisper-server was not produced in ${build}`);
  copyFileSync(built, target);
  writeFileSync(stamp, `${buildId}\n`);
}
process.stdout.write(`${target}\n`);
