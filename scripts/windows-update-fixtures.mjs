// Prepare source-identical controlled updater builds without changing the main
// checkout or generating signing keys. Native build/sign/install is separate.
import { readFileSync, writeFileSync, mkdirSync, copyFileSync, lstatSync, realpathSync } from 'node:fs';
import { resolve, dirname, relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { sourceIdentity } from './windows-qualify.mjs';
import { syncVersion } from './release-version.mjs';

export function updateEndpoint(value) {
  const url = new URL(value);
  if (url.protocol !== 'https:' || url.hostname !== '127.0.0.1' || !/^\d+$/.test(url.port) || Number(url.port) < 1024 || url.username || url.password || url.search || url.hash || url.pathname !== '/update') throw new Error('Use a trusted TLS loopback endpoint such as https://127.0.0.1:17420/update.');
  return url.href;
}

export function prepareUpdateFixtures(root, destination, endpoint) {
  endpoint = updateEndpoint(endpoint);
  const tmp = realpathSync(resolve(root, 'tmp')), parent = realpathSync(dirname(destination)), rel = relative(tmp, parent);
  if (rel.startsWith('..') || isAbsolute(rel)) throw new Error('Use a fresh output directory beneath the existing checkout tmp tree.');
  const source = sourceIdentity(root);
  if (source.files.length > 10000) throw new Error('Unexpected source-copy volume; inspect scope before preparing fixtures.');
  mkdirSync(destination);
  const prepared = [];
  for (const version of ['0.9.0', '1.0.0', '1.1.0']) {
    const workspace = resolve(destination, version, 'workspace'); mkdirSync(workspace, { recursive: true });
    for (const [file] of source.files) {
      const input = resolve(root, file);
      if (lstatSync(input).isSymbolicLink() || lstatSync(input).size > 64 * 1024 * 1024) throw new Error('Fixture source copy refuses symlinks or unexpectedly large individual inputs.');
      const output = resolve(workspace, file); mkdirSync(dirname(output), { recursive: true }); copyFileSync(input, output);
    }
    copyFileSync(resolve(root, '.gitignore'), resolve(workspace, '.gitignore'));
    // Metadata-only test checkout: no stage, commit, tag, remote or publication.
    const git = spawnSync('git', ['init', '--quiet'], { cwd: workspace, encoding: 'utf8', timeout: 30_000 });
    if (git.status !== 0 || sourceIdentity(workspace).hash !== source.hash) throw new Error('Copied fixture source differs; inspect local Git ignore rules/input files.');
    syncVersion(workspace, version);
    const configPath = resolve(workspace, 'src-tauri/tauri.conf.json'), config = JSON.parse(readFileSync(configPath));
    if (!config.plugins?.updater?.pubkey) throw new Error('Existing updater public key is required; never substitute a generated pair.');
    config.plugins.updater.endpoints = [endpoint]; writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);
    const identity = sourceIdentity(workspace);
    if (identity.updateFixtureHash !== source.updateFixtureHash) throw new Error('Fixture changed more than app versions and the controlled update endpoint.');
    const entry = { version, workspace: relative(root, workspace).replaceAll('\\', '/'), sourceHash: identity.hash, updateFixtureHash: identity.updateFixtureHash, buildReceipt: null, nativeBuild: 'PENDING' };
    writeFileSync(resolve(destination, version, 'source.json'), JSON.stringify(entry, null, 2)); prepared.push(entry);
  }
  if (sourceIdentity(root).hash !== source.hash) throw new Error('Main source changed while preparing fixtures.');
  writeFileSync(resolve(destination, 'fixtures.json'), JSON.stringify({ originalSource: source, endpoint, prepared, status: 'SOURCE-PREPARED — native signed builds and installed updater cases pending' }, null, 2));
  return prepared;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 4) throw new Error('Usage: node scripts/windows-update-fixtures.mjs tmp/<fresh-fixture-set> https://127.0.0.1:17420/update');
    const root = fileURLToPath(new URL('..', import.meta.url));
    prepareUpdateFixtures(root, resolve(process.argv[2]), process.argv[3]);
    console.log('Three controlled source workspaces prepared; build each natively with the existing signing credentials. No engine or key was created.');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
