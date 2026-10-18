// Builds helper/ (Java, run on the phone with app_process) into
// src-tauri/resources/helper/adbkeep-helper.dex. Needs a JDK and the Android SDK with
// platforms;android-36 and build-tools;36.0.0 (ANDROID_HOME or ~/Android/Sdk).
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, rmSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { homedir, tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const sdk = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT ?? join(homedir(), "Android", "Sdk");
const androidJar = join(sdk, "platforms", "android-36", "android.jar");
const d8 = join(sdk, "build-tools", "36.0.0", process.platform === "win32" ? "d8.bat" : "d8");
for (const p of [androidJar, d8]) {
  if (!existsSync(p)) {
    console.error(`Missing ${p}: install platforms;android-36 and build-tools;36.0.0`);
    process.exit(1);
  }
}

const work = join(tmpdir(), `adbkeep-helper-${process.pid}`);
const classes = join(work, "classes");
mkdirSync(classes, { recursive: true });

function javaFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? javaFiles(join(dir, e.name)) : e.name.endsWith(".java") ? [join(dir, e.name)] : [],
  );
}
function classFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? classFiles(join(dir, e.name)) : e.name.endsWith(".class") ? [join(dir, e.name)] : [],
  );
}

const sources = javaFiles(join(root, "helper", "src"));
execFileSync("javac", ["-source", "8", "-target", "8", "-Xlint:-options", "-bootclasspath", androidJar, "-d", classes, ...sources], {
  stdio: "inherit",
});
// Android 11 (API 30) is the oldest version adbkeep supports.
execFileSync(d8, ["--release", "--min-api", "26", "--lib", androidJar, "--output", work, ...classFiles(classes)], {
  stdio: "inherit",
});
const outDir = join(root, "src-tauri", "resources", "helper");
mkdirSync(outDir, { recursive: true });
copyFileSync(join(work, "classes.dex"), join(outDir, "adbkeep-helper.dex"));
rmSync(work, { recursive: true, force: true });
console.log(`Built ${join(outDir, "adbkeep-helper.dex")}`);
