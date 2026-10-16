import test from 'node:test';
import assert from 'node:assert/strict';
import { cargoArtifacts } from './measure-build.mjs';
test('cache evidence requires real successful build artifacts and reports each recompiled package', () => {
  const message = fresh => [JSON.stringify({ reason: 'compiler-artifact', package_id: 'fixture@1', fresh }), JSON.stringify({ reason: 'build-finished', success: true })].join('\n');
  assert.deepEqual(cargoArtifacts(message(true)), { count: 1, fresh: 1, rebuilt: [] });
  assert.deepEqual(cargoArtifacts(message(false)).rebuilt, ['fixture@1']);
  for (const text of ['', '{}', 'not json', message(true).replace('"success":true', '"success":false')]) assert.throws(() => cargoArtifacts(text));
});
