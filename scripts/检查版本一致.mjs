import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(
  readFileSync(path.join(root, "package.json"), "utf8"),
);
const tauriConfig = JSON.parse(
  readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"),
);
const cargoExecutable = process.platform === "win32" ? "cargo.exe" : "cargo";
const cargoMetadata = JSON.parse(
  execFileSync(
    cargoExecutable,
    [
      "metadata",
      "--manifest-path",
      path.join(root, "src-tauri", "Cargo.toml"),
      "--no-deps",
      "--format-version",
      "1",
      "--locked",
    ],
    { encoding: "utf8" },
  ),
);
const cargoPackage = cargoMetadata.packages.find(
  (entry) => entry.name === "zongce-records",
);

if (!cargoPackage) {
  throw new Error("Cargo 元数据中缺少 zongce-records 包");
}

const versions = new Map([
  ["package.json", packageJson.version],
  ["src-tauri/Cargo.toml", cargoPackage.version],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
]);
const expectedVersion = packageJson.version;
const mismatches = [...versions].filter(
  ([, version]) => version !== expectedVersion,
);

if (mismatches.length > 0) {
  const details = [...versions]
    .map(([source, version]) => `${source}: ${version}`)
    .join("\n");
  throw new Error(`应用版本不一致：\n${details}`);
}

const tag = process.argv[2];
if (tag && tag !== `v${expectedVersion}`) {
  throw new Error(`发布标签 ${tag} 与应用版本 v${expectedVersion} 不一致`);
}

console.log(tag ? `版本检查通过：${tag}` : `版本检查通过：${expectedVersion}`);
