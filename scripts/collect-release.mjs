import { copyFile, mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';

const target = path.resolve('src-tauri/target/release');
const output = path.resolve('release');
await mkdir(output, { recursive: true });
const binaries = [[path.join(target, 'pi-session-manager.exe'), 'Pi Sessions.exe']];
for (const name of await readdir(path.join(target, 'bundle/nsis'))) {
  if (name.endsWith('.exe')) binaries.push([path.join(target, 'bundle/nsis', name), name]);
}
const hashes = [];
for (const [source, name] of binaries) {
  const destination = path.join(output, name);
  await copyFile(source, destination);
  hashes.push(`${createHash('sha256').update(await readFile(destination)).digest('hex')}  ${name}`);
  console.log(destination);
}
await writeFile(path.join(output, 'SHA256SUMS.txt'), hashes.join('\n') + '\n');
