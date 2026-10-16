import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { prepareSeed, runSeed, recoverSeed } from './fault-seed.mjs';

function fixture() {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'fault-'));
  writeFileSync(resolve(root, 'rule.mjs'), 'export const allowed = false;\r\n');
  writeFileSync(resolve(root, 'check.mjs'), "import assert from 'node:assert/strict'; import { allowed } from './rule.mjs'; assert.equal(allowed, false);\n");
  return { root, original: readFileSync(resolve(root, 'rule.mjs')), seed: { id: 'refusal', file: 'rule.mjs', from: 'allowed = false', to: 'allowed = true', command: 'node', args: ['check.mjs'], case: 'harness-only', intent: 'the runner detects a real assertion failure and restores exact bytes' } };
}
test('real faulted test fails for the protected intent and original CRLF bytes are restored', async () => {
  const f = fixture();
  try {
    const state = await runSeed(f.root, f.seed, resolve(f.root, 'tmp/seed'));
    assert.equal(state.status, 'KILLED'); assert.equal(state.restored, true);
    assert.deepEqual(readFileSync(resolve(f.root, 'rule.mjs')), f.original);
    assert.match(readFileSync(resolve(f.root, 'tmp/seed/test.log'), 'utf8'), /AssertionError/);
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});
test('surviving, invalid and timed out commands never count as killed and still restore bytes', async () => {
  const f = fixture();
  try {
    writeFileSync(resolve(f.root, 'pass.mjs'), 'process.exit(0);\n');
    writeFileSync(resolve(f.root, 'invalid.mjs'), 'process.exit(2);\n');
    writeFileSync(resolve(f.root, 'wait.mjs'), 'setTimeout(() => {}, 30000);\n');
    for (const [name, expected, timeout] of [['pass', 'SURVIVED', 5000], ['invalid', 'INVALID', 5000], ['wait', 'TIMEOUT', 250]]) {
      const state = await runSeed(f.root, { ...f.seed, id: name, args: [`${name}.mjs`] }, resolve(f.root, `tmp/${name}`), timeout);
      assert.equal(state.status, expected); assert.equal(state.restored, true);
      assert.deepEqual(readFileSync(resolve(f.root, 'rule.mjs')), f.original);
    }
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});
test('anchors, outside paths and concurrent edits are refused without overwriting unrelated work', async () => {
  const f = fixture();
  try {
    assert.throws(() => prepareSeed(f.root, { ...f.seed, from: 'absent' }));
    assert.throws(() => prepareSeed(f.root, { ...f.seed, file: '../outside' }));
    writeFileSync(resolve(f.root, 'edit.mjs'), "import { writeFileSync } from 'node:fs'; writeFileSync('rule.mjs', 'concurrent work');\n");
    const dir = resolve(f.root, 'tmp/conflict');
    await assert.rejects(runSeed(f.root, { ...f.seed, args: ['edit.mjs'] }, dir), /concurrently/);
    assert.equal(readFileSync(resolve(f.root, 'rule.mjs'), 'utf8'), 'concurrent work');
    assert.throws(() => recoverSeed(f.root, dir), /conflict/);
    const prepared = prepareSeed(f.root, { ...f.seed, from: 'concurrent work', to: 'own mutation' });
    assert.equal(prepared.original.toString(), 'concurrent work');
    // Arrange a real interrupted-owned state, then prove exact recovery.
    writeFileSync(resolve(f.root, 'rule.mjs'), 'export const allowed = true;\r\n');
    recoverSeed(f.root, dir);
    assert.deepEqual(readFileSync(resolve(f.root, 'rule.mjs')), f.original);
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});

test('a changed backup refuses restoration even when the command failed', async () => {
  const f = fixture();
  try {
    const dir = resolve(f.root, 'tmp/backup-conflict');
    writeFileSync(resolve(f.root, 'corrupt.mjs'), "import { writeFileSync } from 'node:fs'; writeFileSync(process.argv[2], 'changed backup'); process.exitCode = 2;\n");
    await assert.rejects(runSeed(f.root, { ...f.seed, args: ['corrupt.mjs', resolve(dir, 'source.backup')] }, dir), /Backup changed/);
    const state = JSON.parse(readFileSync(resolve(dir, 'state.json'), 'utf8'));
    assert.equal(state.status, 'BACKUP-CONFLICT'); assert.equal(state.restored, false);
    assert.equal(readFileSync(resolve(f.root, 'rule.mjs'), 'utf8'), 'export const allowed = true;\r\n');
    assert.equal(readFileSync(resolve(dir, 'source.backup'), 'utf8'), 'changed backup');
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});
