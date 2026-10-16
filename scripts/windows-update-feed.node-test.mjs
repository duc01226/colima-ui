import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { UPDATE_VARIANTS, signedFixture, updateOffer, updateHandler } from './windows-update-feed.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const origin = 'https://127.0.0.1:17420';
const fixtures = {
  baseline: { version: '1.0.0', signature: 'opaque-baseline-signature' },
  newer: { version: '1.1.0', signature: 'opaque-newer-signature' },
  older: { version: '0.9.0', signature: 'opaque-older-signature' },
};

test('each controlled offer retains its actual version and signature except the explicitly defective field', () => {
  for (const variant of UPDATE_VARIANTS) {
    const offer = updateOffer(variant, fixtures, origin);
    const selected = variant === 'same' ? fixtures.baseline : variant === 'older' ? fixtures.older : fixtures.newer;
    assert.equal(offer.version, variant === 'malformed' ? 'not-a-version' : selected.version);
    assert.equal(offer.platforms['windows-x86_64'].signature, variant === 'wrong-signature' ? fixtures.baseline.signature : selected.signature);
    assert.deepEqual(offer.platforms['windows-x86_64-nsis'], offer.platforms['windows-x86_64']);
    assert.equal(offer.platforms['windows-x86_64'].url, `${origin}/installer.exe`);
  }
  assert.throws(() => updateOffer('unknown', fixtures, origin));
});

test('real feed delivery changes only the tampered response, truncates interruption, and never modifies signed inputs', async () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'update-feed-'));
  const bytes = Buffer.alloc(131077, 71), path = resolve(root, 'installer.exe'); writeFileSync(path, bytes);
  try {
    const files = Object.fromEntries(Object.entries(fixtures).map(([key, value]) => [key, { ...value, path, size: bytes.length }]));
    for (const variant of UPDATE_VARIANTS) {
      const logs = [];
      // HTTP exercises the real handler/stream boundary only. Native trusted
      // HTTPS and updater signature enforcement remain qualification cases.
      const server = createServer(updateHandler(variant, files, origin, entry => logs.push(entry)));
      server.listen(0, '127.0.0.1'); await once(server, 'listening');
      const url = `http://127.0.0.1:${server.address().port}`;
      try {
        const response = await fetch(`${url}/update`), offer = await response.json();
        assert.equal(response.status, 200);
        assert.deepEqual(offer, updateOffer(variant, files, origin));
        assert.equal((await fetch(`${url}/missing`)).status, 404);
        assert.equal((await fetch(`${url}/update`, { method: 'POST' })).status, 405);
        if (variant === 'interrupted') await assert.rejects(async () => {
          const download = await fetch(`${url}/installer.exe`);
          await download.arrayBuffer();
        });
        else {
          const download = await fetch(`${url}/installer.exe`);
          assert.equal(Number(download.headers.get('content-length')), bytes.length);
          const actual = Buffer.from(await download.arrayBuffer()), expected = Buffer.from(bytes);
          if (variant === 'tampered') expected[expected.length - 1] ^= 1;
          assert.deepEqual(actual, expected);
        }
        assert.deepEqual(readFileSync(path), bytes);
        assert.ok(logs.length >= 2 && logs.every(entry => entry.deliveryOnly === true));
      } finally { server.closeAllConnections(); await new Promise(done => { server.close(done); }); }
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('feed fixtures refuse wrong source/version, missing signatures and changed installer bytes', async () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'update-receipt-'));
  try {
    const installer = 'src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/fixture-setup.exe';
    const path = resolve(root, installer), receipt = resolve(root, 'tmp/reports/windows-build/build.json');
    mkdirSync(dirname(path), { recursive: true }); mkdirSync(dirname(receipt), { recursive: true });
    const bytes = Buffer.from('synthetic artifact; never an executable or native signature proof');
    const signature = Buffer.from('opaque-test-signature'); writeFileSync(path, bytes); writeFileSync(`${path}.sig`, signature);
    const build = { version: '1.1.0', mode: 'signed', target: 'x86_64-pc-windows-msvc', source: { updateFixtureHash: 'same-family' }, authenticode: Array.from({ length: 2 }, () => ({ status: 'Valid', timestampThumbprint: 'synthetic-metadata' })), artifacts: [{ file: installer, sha256: hash(bytes) }, { file: `${installer}.sig`, sha256: hash(signature) }] };
    const save = value => writeFileSync(receipt, JSON.stringify(value)); save(build);
    assert.equal((await signedFixture(receipt, '1.1.0', 'same-family')).installerSha256, hash(bytes));
    await assert.rejects(signedFixture(receipt, '1.0.0', 'same-family'));
    await assert.rejects(signedFixture(receipt, '1.1.0', 'other-family'));
    save({ ...build, artifacts: build.artifacts.slice(0, 1) });
    await assert.rejects(signedFixture(receipt, '1.1.0', 'same-family'));
    save(build); writeFileSync(path, Buffer.from('changed bytes'));
    await assert.rejects(signedFixture(receipt, '1.1.0', 'same-family'), /hash changed/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
