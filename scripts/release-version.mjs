// One semantic application version for frontend, native runtime and updater.
// OS-specific numeric file-version projection belongs to the Tauri bundler.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const identifier = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)';
const semver = new RegExp(`^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(?:-${identifier}(?:\\.${identifier})*)?(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$`);
const rootVersion = /(^\[package\]\r?\n(?:(?!\[)[^\n]*\n)*?version = ")[^"]+("[^\S\r\n]*(?:#[^\r\n]*)?\r?$)/m;
const ownPackage = /(^\[\[package\]\]\r?\nname = "colima-ui"\r?\nversion = ")[^"]+("\r?$)/gm;

export function applicationVersions(root) {
  const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
  const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json'), 'utf8'));
  const tauri = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  const cargo = readFileSync(resolve(root, 'src-tauri/Cargo.toml'), 'utf8').match(rootVersion)?.[0];
  const locked = [...readFileSync(resolve(root, 'src-tauri/Cargo.lock'), 'utf8').matchAll(ownPackage)];
  if (!cargo || locked.length !== 1) throw new Error('Missing/ambiguous Cargo application version.');
  return [pkg.version, lock.version, lock.packages?.['']?.version, tauri.version,
    cargo.match(/version = "([^"]+)"/)?.[1], locked[0][0].match(/version = "([^"]+)"/)?.[1]].map(validateVersion);
}

export function validateVersion(version) {
  if (typeof version !== 'string' || version !== version.trim() || !semver.test(version)) {
    throw new Error('Expected a complete SemVer version, for example 1.2.3-rc.1.');
  }
  return version;
}

export function versionUpdates(root, version) {
  validateVersion(version);
  // Parse and validate every carrier before the first write. Never match a
  // dependency's version or drop the prerelease suffix to satisfy one platform.
  const updates = new Map();
  for (const name of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
    const path = resolve(root, name);
    const data = JSON.parse(readFileSync(path, 'utf8'));
    data.version = version;
    if (name === 'package-lock.json') {
      if (!data.packages?.['']) throw new Error('npm lock has no root package.');
      data.packages[''].version = version;
    }
    updates.set(path, `${JSON.stringify(data, null, 2)}\n`);
  }
  const cargo = resolve(root, 'src-tauri/Cargo.toml');
  const manifest = readFileSync(cargo, 'utf8');
  if (!rootVersion.test(manifest)) {
    throw new Error('Cargo manifest has no root package version.');
  }
  updates.set(cargo, manifest.replace(rootVersion, (_match, start, end) => `${start}${version}${end}`));
  const lock = resolve(root, 'src-tauri/Cargo.lock');
  const locked = readFileSync(lock, 'utf8');
  if ([...locked.matchAll(ownPackage)].length !== 1) throw new Error('Cargo lock must identify exactly one colima-ui package.');
  updates.set(lock, locked.replace(ownPackage, (_match, start, end) => `${start}${version}${end}`));
  return updates;
}

export function syncVersion(root, version) {
  for (const [path, contents] of versionUpdates(root, version)) writeFileSync(path, contents);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node scripts/release-version.mjs <version>');
    syncVersion(fileURLToPath(new URL('..', import.meta.url)), process.argv[2]);
    console.log(`Application version synchronized: ${process.argv[2]}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
