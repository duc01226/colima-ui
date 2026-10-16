import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { prepareUpdateFixtures, updateEndpoint } from './windows-update-fixtures.mjs';
import { applicationVersions } from './release-version.mjs';
import { updateFixtureInput } from './windows-qualify.mjs';

test('controlled feed stays trusted HTTPS loopback without weakening updater public-key identity', () => {
  assert.equal(updateEndpoint('https://127.0.0.1:17420/update'), 'https://127.0.0.1:17420/update');
  for (const url of ['http://127.0.0.1:17420/update', 'https://example.com:17420/update', 'https://user:pass@127.0.0.1:17420/update', 'https://127.0.0.1:17420/update?token=private']) assert.throws(() => updateEndpoint(url));
  const config = { version: '0.0.0-fixture', plugins: { updater: { pubkey: 'retained-public-key', endpoints: ['https://production.invalid'] } } };
  const normalized = updateFixtureInput('src-tauri/tauri.conf.json', JSON.stringify(config));
  assert.equal(updateFixtureInput('src-tauri/tauri.conf.json', JSON.stringify({ ...config, plugins: { updater: { ...config.plugins.updater, endpoints: ['https://127.0.0.1:17420/update'] } } })), normalized);
  config.plugins.updater.pubkey = 'wrong-public-key';
  assert.notEqual(updateFixtureInput('src-tauri/tauri.conf.json', JSON.stringify(config)), normalized);
});

test('real controlled workspaces change only version/endpoint, preserve main source, and never overwrite prior fixtures', () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'updater-source-'));
  try {
    for (const dir of ['tmp', 'src-tauri', 'docs/specs', 'src']) mkdirSync(resolve(root, dir), { recursive: true });
    writeFileSync(resolve(root, '.gitignore'), '/tmp/\n');
    writeFileSync(resolve(root, 'package.json'), JSON.stringify({ version: '0.2.0' }));
    writeFileSync(resolve(root, 'package-lock.json'), JSON.stringify({ version: '0.2.0', packages: { '': { version: '0.2.0' } } }));
    writeFileSync(resolve(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ version: '0.2.0', plugins: { updater: { pubkey: 'fixture-public-key', endpoints: ['https://production.invalid'] } } }));
    writeFileSync(resolve(root, 'src-tauri/Cargo.toml'), '[package]\nname = "colima-ui"\nversion = "0.2.0"\n');
    writeFileSync(resolve(root, 'src-tauri/Cargo.lock'), 'version = 4\n\n[[package]]\nname = "colima-ui"\nversion = "0.2.0"\n');
    writeFileSync(resolve(root, 'docs/specs/windows-docker-ui.md'), '# Synthetic source identity fixture\nPreserve data.\n');
    writeFileSync(resolve(root, 'src/preserved.ts'), 'export const retained = true;\n');
    assert.equal(spawnSync('git', ['init', '--quiet'], { cwd: root }).status, 0);
    const before = readFileSync(resolve(root, 'package.json'));
    const output = resolve(root, 'tmp/fixtures');
    const entries = prepareUpdateFixtures(root, output, 'https://127.0.0.1:17420/update');
    assert.deepEqual(readFileSync(resolve(root, 'package.json')), before);
    assert.equal(entries.length, 3);
    assert.equal(new Set(entries.map(e => e.updateFixtureHash)).size, 1);
    for (const entry of entries) {
      assert.ok(applicationVersions(resolve(root, entry.workspace)).every(version => version === entry.version));
      assert.equal(readFileSync(resolve(root, entry.workspace, 'src/preserved.ts'), 'utf8'), 'export const retained = true;\n');
    }
    assert.throws(() => prepareUpdateFixtures(root, output, 'https://127.0.0.1:17420/update'));
  } finally { rmSync(root, { recursive: true, force: true }); }
});
