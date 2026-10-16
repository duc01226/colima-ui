import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { syncVersion, validateVersion, versionUpdates } from './release-version.mjs';

function fixture(t) {
  mkdirSync('tmp', { recursive: true });
  const root = mkdtempSync(join('tmp', 'version-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'src-tauri'));
  writeFileSync(join(root, 'package.json'), JSON.stringify({ version: '0.1.0' }));
  writeFileSync(join(root, 'package-lock.json'), JSON.stringify({ version: '0.1.0', packages: { '': { version: '0.1.0' }, 'node_modules/other': { version: '9.8.7' } } }));
  writeFileSync(join(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ version: '0.1.0', identifier: 'com.colima.ui' }));
  writeFileSync(join(root, 'src-tauri/Cargo.toml'), '[package]\nname = "colima-ui"\nversion = "0.1.0"\n\n[dependencies]\nother = { version = "9.8.7" }\n');
  writeFileSync(join(root, 'src-tauri/Cargo.lock'), 'version = 4\n\n[[package]]\nname = "colima-ui"\nversion = "0.1.0"\n\n[[package]]\nname = "other"\nversion = "9.8.7"\n');
  return root;
}

// Intent TC-WINUI-023: runtime/updater/display agree on the declared version,
// including prereleases; a dependency must never receive the app's version.
for (const version of ['1.2.3', '1.2.3-rc.1', '1.2.3-beta.0+build.7']) {
  test(`all application carriers retain ${version} without dependency drift`, t => {
    const root = fixture(t);
    syncVersion(root, version);
    for (const path of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
      const data = JSON.parse(readFileSync(join(root, path)));
      assert.equal(data.version, version);
      if (data.packages) {
        assert.equal(data.packages[''].version, version);
        assert.equal(data.packages['node_modules/other'].version, '9.8.7');
      }
    }
    const manifest = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8');
    const lock = readFileSync(join(root, 'src-tauri/Cargo.lock'), 'utf8');
    assert.ok(manifest.includes(`name = "colima-ui"\nversion = "${version}"`));
    assert.ok(manifest.includes('other = { version = "9.8.7" }'));
    assert.ok(lock.includes(`name = "colima-ui"\nversion = "${version}"`));
    assert.ok(lock.includes('name = "other"\nversion = "9.8.7"'));
  });
}

test('malformed or shell-bearing versions are refused before any write', t => {
  const root = fixture(t);
  const before = readFileSync(join(root, 'package.json'));
  for (const version of ['v1.2.3', '01.2.3', '1.2.3-01', '1.2.3-', '1.2', 'not-a-version', '1.2.3;echo bad', '1.2.3\n', undefined]) {
    assert.throws(() => validateVersion(version));
    assert.throws(() => syncVersion(root, version));
    assert.deepEqual(readFileSync(join(root, 'package.json')), before);
  }
});

test('controlled update identity can prepare complete version-only substitutions without writing source', t => {
  const root = fixture(t), path = join(root, 'package.json'), before = readFileSync(path);
  const updates = versionUpdates(root, '0.0.0-fixture');
  assert.equal(updates.size, 5);
  assert.deepEqual(readFileSync(path), before);
  assert.ok([...updates.values()].every(value => value.includes('0.0.0-fixture')));
  assert.ok([...updates.values()].some(value => value.includes('9.8.7')));
});

test('an incomplete lock cannot leave earlier manifests partially synchronized', t => {
  const root = fixture(t);
  const path = join(root, 'package.json');
  const before = readFileSync(path);
  writeFileSync(join(root, 'src-tauri/Cargo.lock'), 'version = 4\n');
  assert.throws(() => syncVersion(root, '1.1.0'), /exactly one/);
  assert.deepEqual(readFileSync(path), before);
});
