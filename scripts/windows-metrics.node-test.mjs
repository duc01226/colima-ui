import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { metricsOptions, assertMetricsReceipt } from './windows-metrics.mjs';

const args = ['--provider', 'docker-desktop', '--context', 'desktop-linux', '--engine-id', 'fixture', '--fixture-image', 'fixture:v1', '--fixture-id', `sha256:${'a'.repeat(64)}`, '--dedicated-engine', 'fixture', '--profile', 'disposable-account', '--app', 'C:\\Program Files\\ColimaUI\\colima-ui.exe', '--build-receipt', 'build.json', '--count', '200'];
test('installed metrics requires dedicated provider input, exact artifact and independent fixed workload size', () => {
  assert.equal(metricsOptions(args).count, '200');
  for (const count of ['0', '20,200', '201']) assert.throws(() => metricsOptions([...args.slice(0, -1), count]));
  assert.throws(() => metricsOptions(args.slice(0, -4)));
  assert.throws(() => metricsOptions([...args, '--count', '20']));
});

// Synthetic validator fixture only; never emitted as native qualification.
function example() {
  const ticks = Array.from({ length: 30 }, (_, i) => ({ sequence: i + 1, durationMs: 1, samples: 200, failed: false }));
  return { result: 'PASS', runId: 'fixture-run', providerDeclared: 'docker-desktop', context: 'desktop-linux', engineFingerprint: createHash('sha256').update('fixture').digest('hex'), appSha256: 'a'.repeat(64), appVersion: '0.2.0', processId: 123, workloads: 200, intervalMs: 2000, idleBeforeAndAfter: true, ownedCleanup: true,
    normal: { cpuSeconds: 0.1, wallSeconds: 60, ticks }, recovered: { cpuSeconds: 0.1, wallSeconds: 60, ticks: ticks.map(t => ({ ...t, sequence: t.sequence + 60 })) },
    points: Array.from({ length: 200 }, (_, i) => ({ elapsedMs: i * 1000, cpuSeconds: i / 1000, residentBytes: 1000, liveProcesses: 2, totalProcesses: 2 })), baselineStartMs: 0, firstFleetCreationMs: 60_000, peakEndMs: 190_000, baselineResidentBytes: 1000, peakResidentBytes: 1000, residentGrowthBytes: 0 };
}

test('receipt validation recomputes budgets and rejects stale, partial, missing or reassigned process evidence', () => {
  const options = metricsOptions(args);
  const validate = receipt => assertMetricsReceipt(receipt, options, 'fixture-run', 'a'.repeat(64), '0.2.0');
  assert.doesNotThrow(() => validate(example()));
  const changes = [
    r => { r.runId = 'old'; }, r => { r.appSha256 = 'other'; }, r => { r.providerDeclared = 'rancher-moby'; },
    r => { r.normal.cpuSeconds = 3; }, r => { r.recovered.ticks[2].failed = true; }, r => { r.normal.ticks[2].samples = 199; },
    r => { r.normal.ticks = []; }, r => { r.normal.ticks.forEach(t => { t.durationMs = 2000; }); },
    r => { r.points.splice(30, 1); }, r => { r.points[12].residentBytes = 0; },
    r => { r.firstFleetCreationMs = 59_000; }, r => { r.peakResidentBytes = 10; },
    r => { r.points[90].residentBytes = 268436456; r.peakResidentBytes = 268436456; r.residentGrowthBytes = 268435456; },
    r => { r.idleBeforeAndAfter = false; }, r => { r.ownedCleanup = false; },
  ];
  for (const change of changes) { const receipt = example(); change(receipt); assert.throws(() => validate(receipt)); }
});
