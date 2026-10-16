import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { qualificationOptions, assertBoundaryReceipt } from './windows-qualify.mjs';

const values = ['--provider', 'docker-desktop', '--context', 'desktop-linux', '--engine-id', 'fixture-engine', '--fixture-image', 'fixture:v1', '--fixture-id', `sha256:${'a'.repeat(64)}`, '--dedicated-engine', 'fixture-engine', '--profile', 'disposable-account'];

test('destructive native tier requires explicit exact engine, known provider and disposable profile', () => {
  assert.equal(qualificationOptions(values).context, 'desktop-linux');
  for (const [before, after] of [['fixture-engine', 'another-engine'], ['disposable-account', 'personal-account'], ['docker-desktop', 'podman'], ['desktop-linux', '--host=other']]) {
    const altered = [...values];
    altered[altered.indexOf(before)] = after;
    assert.throws(() => qualificationOptions(altered));
  }
  assert.throws(() => qualificationOptions(values.slice(0, -2)));
  assert.throws(() => qualificationOptions([...values, '--context', 'default']));
});

test('a successful harness exit or stale/partial receipt cannot replace actual current engine-bound assertions', () => {
  const options = qualificationOptions(values);
  const receipt = { result: 'PASS', runId: 'fixture-run', providerDeclared: options.provider, context: options.context, engineFingerprint: createHash('sha256').update(options['engine-id']).digest('hex'), asserted: ['containers', 'images', 'volumes', 'networks', 'copy hash/TAR roundtrip', 'image export/import', 'Compose Unicode file', 'ConPTY UTF8/resize/interrupt/exit', 'terminal external ownership', 'metrics attribution', 'owned cleanup'] };
  assert.doesNotThrow(() => assertBoundaryReceipt(receipt, options, 'fixture-run'));
  for (const change of [{ result: 'SKIP' }, { runId: 'old-run' }, { engineFingerprint: 'other' }, { asserted: [] }, { providerDeclared: 'rancher-moby' }]) {
    assert.throws(() => assertBoundaryReceipt({ ...receipt, ...change }, options, 'fixture-run'));
  }
});
