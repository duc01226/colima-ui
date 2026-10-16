import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { canonicalCases, verifiedArtifact, validateCase, validateBundle, validateGates } from './windows-evidence.mjs';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const source = readFileSync(new URL('../docs/specs/windows-docker-ui.md', import.meta.url), 'utf8');

// Shared input fixture only; the production validator remains the independent
// owner of acceptance checks. A new update control changes one fixture table.
function updateVariants(reference) {
  return [
    ['newer', '1.1.0', '1.1.0', 'updated'],
    ['same', '1.0.0', '1.0.0', 'no-update'],
    ['older', '0.9.0', '1.0.0', 'no-update'],
    ['malformed', 'not-a-version', '1.0.0', 'failure'],
    ['tampered', '1.1.0', '1.0.0', 'failure'],
    ['wrong-signature', '1.1.0', '1.0.0', 'failure'],
    ['interrupted', '1.1.0', '1.0.0', 'failure'],
  ].map(([variant, offeredVersion, installedVersion, outcome]) => ({
    variant, baselineVersion: '1.0.0', offeredVersion, installedVersion, outcome,
    usableAfterRelaunch: true, stateBeforeSha256: 'a'.repeat(64),
    stateAfterSha256: 'a'.repeat(64), installedReadback: reference,
  }));
}

// A complete synthetic worksheet exercises the structural validator's real file
// and hash boundaries. It is never native observation or signature proof, and
// is never written as bundle.json or an acceptance/structural-validation result.
function completeBundle(root) {
  const currentSource = 'c'.repeat(64), appVersion = '0.2.0';
  const artifact = (name, value, role = 'synthetic-control') => {
    const bytes = typeof value === 'string' ? value : JSON.stringify(value);
    writeFileSync(resolve(root, name), bytes);
    return { role, path: name, sha256: hash(bytes) };
  };
  const rewrite = (reference, change) => {
    const value = JSON.parse(readFileSync(resolve(root, reference.path), 'utf8'));
    change(value);
    Object.assign(reference, artifact(reference.path, value, reference.role));
  };
  const positive = artifact('positive.txt', 'Synthetic positive observation, not native proof.', 'positive-control');
  const boundary = artifact('boundary.txt', 'Synthetic rejected observation, not native proof.', 'boundary-control');
  const cases = canonicalCases(source);
  const updates = updateVariants(positive);
  const observed = expected => ({ ...expected, status: 'PASS', reviewer: 'synthetic-fixture-only', observedAt: '2026-10-15', arrangement: 'Synthetic validator input', action: 'Synchronous validator only', observedOutcome: 'Fixture, not native acceptance', preservationOutcome: 'Fixture, not resource observation', artifacts: [positive, boundary], ...(expected.id === 'TC-WINUI-023' ? { updateVariants: structuredClone(updates) } : {}) });
  const assets = ['colima-ui.exe', 'fixture-setup.exe', 'fixture-setup.exe.sig'].map(fileName => ({ ...artifact(fileName, `Synthetic asset ${fileName}`, 'build-asset'), fileName }));
  const appSha256 = assets[0].sha256;
  const build = { mode: 'signed', source: { hash: currentSource }, version: appVersion, target: 'x86_64-pc-windows-msvc',
    authenticode: assets.slice(0, 2).map(item => ({ status: 'Valid', signerThumbprint: 'A'.repeat(40), timestampThumbprint: 'B'.repeat(40), sha256: item.sha256 })),
    artifacts: assets.map(item => ({ file: `src-tauri/target/x86_64-pc-windows-msvc/release/${item.fileName}`, sha256: item.sha256 })) };
  const providers = ['docker-desktop', 'rancher-moby'].map((provider, index) => {
    const engineId = `synthetic-engine-${index}`, context = `synthetic-context-${index}`;
    const runId = `synthetic-run-${index}`, engineFingerprint = hash(engineId);
    const native = artifact(`${provider}-boundary.json`, { result: 'PASS', runId, providerDeclared: provider, context, engineFingerprint, source: { hash: currentSource }, os: { ProductType: 1, Caption: 'Windows 11 fixture' }, asserted: ['containers', 'images', 'volumes', 'networks', 'copy hash/TAR roundtrip', 'image export/import', 'Compose Unicode file', 'ConPTY UTF8/resize/interrupt/exit', 'terminal external ownership', 'metrics attribution', 'owned cleanup'] });
    const metrics = Object.fromEntries([20, 200].map(count => {
      const ticks = offset => Array.from({ length: 30 }, (_, i) => ({ sequence: offset + i, durationMs: 1, samples: count, failed: false }));
      return [String(count), artifact(`${provider}-metrics-${count}.json`, { result: 'PASS', runId: `${runId}-${count}`, providerDeclared: provider, context, engineFingerprint, source: { hash: currentSource }, appSha256, appVersion, processId: 100 + index, workloads: count, intervalMs: 2000, idleBeforeAndAfter: true, ownedCleanup: true,
        normal: { cpuSeconds: 0.1, wallSeconds: 60, ticks: ticks(1) }, recovered: { cpuSeconds: 0.1, wallSeconds: 60, ticks: ticks(61) },
        points: Array.from({ length: 200 }, (_, i) => ({ elapsedMs: i * 1000, residentBytes: 1000, cpuSeconds: i / 1000, liveProcesses: 2, totalProcesses: 2 })), baselineStartMs: 0, firstFleetCreationMs: 60_000, peakEndMs: 190_000, baselineResidentBytes: 1000, peakResidentBytes: 1000, residentGrowthBytes: 0 })];
    }));
    const account = { result: 'PASS', runId: `00000000-0000-0000-0000-00000000000${index}`, ownerSid: 'S-1-5-21-1000', administratorMember: false, referencePathSha256: 'd'.repeat(64), port: 11420 + index, healthStatus: 200 };
    return { provider, engineId, context, providerProvenance: positive, boundary: native, metrics,
      accounts: { owner: artifact(`${provider}-owner.json`, { ...account, role: 'owner', accountSid: account.ownerSid, privateReadable: true, protectedStatus: 200 }), outsider: artifact(`${provider}-outsider.json`, { ...account, role: 'outsider', accountSid: 'S-1-5-21-2000', privateReadable: false, privateDenial: 'UnauthorizedAccess', protectedStatus: 401 }) },
      cases: cases.filter(item => item.id !== 'TC-WINUI-020').map(observed) };
  });
  const bundle = { sourceHash: currentSource, appVersion, appSha256, build: artifact('build-fixture.json', build), assets, providers,
    preservation: ['macos', 'linux'].map(platform => ({ ...observed(cases.find(item => item.id === 'TC-WINUI-020')), platform })),
    gates: Array.from({ length: 25 }, (_, i) => ({ id: `G${i + 1}`, status: [22, 23, 25].includes(i + 1) ? 'N/A' : 'PASS', rationale: 'Synthetic gate, not proof', evidence: positive })) };
  return { bundle, build, rewrite, validate: () => validateBundle(root, bundle, cases, currentSource) };
}

function withBundle(run) {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'complete-bundle-'));
  try { run(completeBundle(root), root); } finally { rmSync(root, { recursive: true, force: true }); }
}

// TechnicalSpec=WINUI-qualification-bundle-bindings: structural source guard
// only; no TestSpec claim of installed/provider behavior or native signatures.
test('a complete two-provider bundle traverses every validator boundary without becoming native acceptance', () => {
  withBundle(({ validate, build }) => assert.deepEqual(validate(), build));
});

for (const [label, mutate, reason] of [
  ['unsigned candidate', f => f.rewrite(f.bundle.build, v => { v.mode = 'candidate'; }), /Signed exact-source/],
  ['another build source', f => f.rewrite(f.bundle.build, v => { v.source.hash = 'e'.repeat(64); }), /Signed exact-source/],
  ['another installed version', f => { f.bundle.appVersion = '0.1.0'; }, /Signed exact-source/],
  ['missing trusted signature', f => f.rewrite(f.bundle.build, v => { v.authenticode[0].status = 'UnknownError'; }), /Trusted timestamped/],
  ['missing exact signed asset', f => { f.bundle.assets.pop(); }, /Missing exact signed build asset/],
  ['one daemon relabelled as two providers', f => { f.bundle.providers[1].engineId = f.bundle.providers[0].engineId; }, /One daemon/],
  ['missing second provider', f => { f.bundle.providers.pop(); }, /rancher-moby/],
  ['different boundary source', f => f.rewrite(f.bundle.providers[0].boundary, v => { v.source.hash = 'e'.repeat(64); }), /current native Windows 11/],
  ['server-host boundary', f => f.rewrite(f.bundle.providers[0].boundary, v => { v.os.ProductType = 3; }), /current native Windows 11/],
  ['different metrics source', f => f.rewrite(f.bundle.providers[0].metrics['20'], v => { v.source.hash = 'e'.repeat(64); }), /Metrics source/],
  ['reused 20-workload run for 200', f => { f.bundle.providers[0].metrics['200'] = f.bundle.providers[0].metrics['20']; }, /current installed-app metrics/],
  ['missing recovered fleet measurement', f => f.rewrite(f.bundle.providers[0].metrics['200'], v => { v.recovered.ticks = []; }), /full metrics window/],
  ['missing native case', f => { f.bundle.providers[0].cases.pop(); }, /all native cases/],
  ['duplicate native case', f => { f.bundle.providers[0].cases[1] = f.bundle.providers[0].cases[0]; }, /all native cases/],
  ['stale canonical outcome', f => { f.bundle.providers[0].cases[0].canonicalSha256 = 'e'.repeat(64); }, /TC-WINUI-001/],
  ['pending native observation', f => { f.bundle.providers[0].cases[0].status = 'PENDING'; }, /TC-WINUI-001/],
  ['missing boundary control', f => { f.bundle.providers[0].cases[0].artifacts = [f.bundle.providers[0].cases[0].artifacts[0]]; }, /TC-WINUI-001/],
  ['owner reused as outsider', f => { f.bundle.providers[0].accounts.outsider = f.bundle.providers[0].accounts.owner; }, /Distinct ordinary/],
  ['privileged outsider', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { v.administratorMember = true; }), /Distinct ordinary/],
  ['readable outsider reference', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { v.privateReadable = true; }), /Distinct ordinary/],
  ['absent file mistaken for access denial', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { v.privateDenial = 'FileNotFound'; }), /Distinct ordinary/],
  ['outsider reaches protected API', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { v.protectedStatus = 200; }), /Distinct ordinary/],
  ['another reference path', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { v.referencePathSha256 = 'e'.repeat(64); }), /Distinct ordinary/],
  ['another account run', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { v.runId = '11111111-1111-1111-1111-111111111111'; }), /same arranged run UUID/],
  ['missing owner run', f => f.rewrite(f.bundle.providers[0].accounts.owner, v => { delete v.runId; }), /same arranged run UUID/],
  ['missing outsider run', f => f.rewrite(f.bundle.providers[0].accounts.outsider, v => { delete v.runId; }), /same arranged run UUID/],
  ['matching malformed runs', f => { for (const ref of Object.values(f.bundle.providers[0].accounts)) f.rewrite(ref, v => { v.runId = 'not-a-guid'; }); }, /same arranged run UUID/],
  ['missing Linux preservation', f => { f.bundle.preservation.pop(); }, /linux:/],
  ['failed preservation', f => { f.bundle.preservation[0].status = 'FAIL'; }, /TC-WINUI-020/],
  ['unverified context parity', f => { f.bundle.gates[23].status = 'N/A'; }, /G24:/],
]) {
  // TechnicalSpec=WINUI-qualification-bundle-bindings: each guard has a full
  // valid positive fixture, so an earlier unrelated rejection cannot fake it.
  test(`complete-bundle validation refuses ${label}`, () => {
    withBundle(fixture => { mutate(fixture); assert.throws(fixture.validate, reason); });
  });
}

test('applicable gates require observed evidence including regenerated context; N/A remains restricted to excluded work', () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'gates-'));
  try {
    const body = 'Synthetic gate-validator fixture, not execution or acceptance evidence.';
    writeFileSync(resolve(root, 'gate.txt'), body);
    const gates = Array.from({ length: 25 }, (_, i) => ({ id: `G${i + 1}`, status: [22, 23, 25].includes(i + 1) ? 'N/A' : 'PASS', rationale: 'Fixture only', evidence: { role: 'gate-observation', path: 'gate.txt', sha256: hash(body) } }));
    assert.doesNotThrow(() => validateGates(root, gates));
    for (const n of [...Array.from({ length: 21 }, (_, i) => i + 1), 24]) {
      const bad = structuredClone(gates); bad[n - 1].status = 'N/A';
      assert.throws(() => validateGates(root, bad), new RegExp(`G${n}:`));
    }
    assert.throws(() => validateGates(root, gates.slice(1)), /All approved/);
    const bad = structuredClone(gates); bad[23].evidence.sha256 = '0'.repeat(64);
    assert.throws(() => validateGates(root, bad), /changed/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('worksheet inventory comes from all canonical cases; recording evidence does not change accepted intent hashes', () => {
  const cases = canonicalCases(source);
  assert.equal(cases.length, 38);
  assert.deepEqual(canonicalCases(source.replaceAll('**CoveredBy:** Untested', '**CoveredBy:** native evidence')), cases);
  assert.throws(() => canonicalCases(source.replace('#### TC-WINUI-001:', '#### OTHER-001:')));
  const altered = canonicalCases(source.replace('only the original verified engine can resume', 'any engine can resume'));
  assert.notDeepEqual(altered, cases);
});

test('evidence must exist inside its bundle, retain exact bytes and exclude private material', () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'evidence-'));
  try {
    const body = 'Synthetic observation fixture only.'; writeFileSync(resolve(root, 'observed.txt'), body);
    const reference = { role: 'positive-control', path: 'observed.txt', sha256: hash(body) };
    assert.equal(verifiedArtifact(root, reference).toString(), body);
    assert.throws(() => verifiedArtifact(root, { ...reference, path: '../outside.txt' }));
    assert.throws(() => verifiedArtifact(root, { ...reference, sha256: '0'.repeat(64) }));
    const secret = 'colima-' + 'a'.repeat(64); writeFileSync(resolve(root, 'private.txt'), secret);
    assert.throws(() => verifiedArtifact(root, { ...reference, path: 'private.txt', sha256: hash(secret) }), /Credential/);
    const expected = canonicalCases(source)[0];
    const record = { ...expected, status: 'PASS', reviewer: 'synthetic-test', observedAt: '2026-10-15', arrangement: 'fixture', action: 'fixture', observedOutcome: 'fixture', preservationOutcome: 'fixture', artifacts: [reference, { ...reference, role: 'boundary-control' }] };
    assert.doesNotThrow(() => validateCase(record, expected, root));
    for (const change of [{ status: 'PENDING' }, { canonicalSha256: '0'.repeat(64) }, { reviewer: '' }, { artifacts: [reference] }]) assert.throws(() => validateCase({ ...record, ...change }, expected, root));
    assert.throws(() => validateBundle(root, { sourceHash: 'stale' }, canonicalCases(source), 'current'));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('every update-negative variant requires separate installed-version and preserved-state readback', () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'updates-'));
  try {
    const body = 'Synthetic installed-version/readback fixture, not native proof.'; writeFileSync(resolve(root, 'readback.txt'), body);
    const reference = { role: 'installed-readback', path: 'readback.txt', sha256: hash(body) };
    const expected = canonicalCases(source).find(item => item.id === 'TC-WINUI-023');
    const variants = updateVariants(reference);
    const record = { ...expected, status: 'PASS', reviewer: 'synthetic-test', observedAt: '2026-10-15', arrangement: 'fixture', action: 'fixture', observedOutcome: 'fixture', preservationOutcome: 'fixture', artifacts: [{ ...reference, role: 'positive-control' }, { ...reference, role: 'boundary-control' }], updateVariants: variants };
    assert.doesNotThrow(() => validateCase(record, expected, root));
    assert.throws(() => validateCase({ ...record, updateVariants: variants.slice(1) }, expected, root));
    for (const outcome of ['updated', 'no-update']) {
      const bad = structuredClone(record); bad.updateVariants.find(v => v.variant === 'malformed').outcome = outcome;
      assert.throws(() => validateCase(bad, expected, root));
    }
    const bad = structuredClone(record); bad.updateVariants[0].installedVersion = '1.0.0';
    assert.throws(() => validateCase(bad, expected, root));
  } finally { rmSync(root, { recursive: true, force: true }); }
});
