import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  writeFile,
} from "node:fs/promises";
import { createHash } from "node:crypto";
import path from "node:path";

const target = path.resolve("src-tauri/target/release");
const output = path.resolve("release");
const { productName, version } = JSON.parse(
  await readFile("src-tauri/tauri.conf.json", "utf8"),
);
const installers = (await readdir(path.join(target, "bundle/nsis"))).filter(
  (name) =>
    name.startsWith(`${productName}_${version}_`) &&
    name.endsWith("-setup.exe"),
);
if (installers.length === 0)
  throw new Error(`No NSIS installer found for ${productName} ${version}`);
await mkdir(output, { recursive: true });
const binaries = [
  [path.join(target, "pi-session-manager.exe"), "Pi Sessions.exe"],
];
for (const name of installers) {
  binaries.push([path.join(target, "bundle/nsis", name), name]);
}
const hashes = [];
for (const [source, name] of binaries) {
  const destination = path.join(output, name);
  await copyFile(source, destination);
  hashes.push(
    `${createHash("sha256")
      .update(await readFile(destination))
      .digest("hex")}  ${name}`,
  );
  console.log(destination);
}
await writeFile(path.join(output, "SHA256SUMS.txt"), hashes.join("\n") + "\n");
