// Downloads Google's platform-tools for the current OS (or the one given as the first
// argument: linux, darwin, windows) into src-tauri/resources/platform-tools, keeping only
// what adbkeep needs. Skips the download when adb is already there; pass --force to redo it.
import { mkdir, rm, writeFile, chmod, access } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { unzipSync } from "fflate";

const osArg = process.argv.slice(2).find((a) => !a.startsWith("--"));
const force = process.argv.includes("--force");
const os = osArg ?? { linux: "linux", darwin: "darwin", win32: "windows" }[process.platform];
if (!["linux", "darwin", "windows"].includes(os)) {
  console.error(`Unsupported OS: ${os}`);
  process.exit(1);
}

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const dest = join(root, "src-tauri", "resources", "platform-tools");
const adb = os === "windows" ? "adb.exe" : "adb";
const keep = new Set([adb, "AdbWinApi.dll", "AdbWinUsbApi.dll", "NOTICE.txt", "source.properties"]);

if (!force) {
  try {
    await access(join(dest, adb));
    console.log(`platform-tools already in ${dest}`);
    process.exit(0);
  } catch {}
}

const url = `https://dl.google.com/android/repository/platform-tools-latest-${os}.zip`;
console.log(`Downloading ${url}`);
const res = await fetch(url);
if (!res.ok) {
  console.error(`Download failed: HTTP ${res.status}`);
  process.exit(1);
}
const files = unzipSync(new Uint8Array(await res.arrayBuffer()), {
  filter: (f) => keep.has(f.name.replace(/^platform-tools\//, "")),
});

await rm(dest, { recursive: true, force: true });
await mkdir(dest, { recursive: true });
for (const [name, data] of Object.entries(files)) {
  const out = join(dest, name.replace(/^platform-tools\//, ""));
  await writeFile(out, data);
  if (name.endsWith(`/${adb}`) && os !== "windows") await chmod(out, 0o755);
  console.log(`  ${out}`);
}
