// Generates a pending worksheet from the canonical owner and validates actual
// local artifact bindings. Structural validation NEVER accepts Windows support
// or authorizes publication; a reviewer must inspect each native case outcome.
import { randomUUID, createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, realpathSync, copyFileSync, lstatSync } from 'node:fs';
import { resolve, relative, isAbsolute, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { sourceIdentity, nativeHost, assertBoundaryReceipt } from './windows-qualify.mjs';
import { assertMetricsReceipt } from './windows-metrics.mjs';
import { secretViolations } from './check-windows-foundation.mjs';
import { qualificationIntent } from './windows-spec.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const digestPattern = /^[a-f0-9]{64}$/;
const providers = ['docker-desktop', 'rancher-moby'];

export function canonicalCases(source) {
  const matches = [...source.matchAll(/^#### (TC-WINUI-\d{3}): (.+)$/gm)];
  const cases = matches.map((match, index) => ({ id: match[1], title: match[2], canonicalSha256: sha(qualificationIntent(source.slice(match.index, matches[index + 1]?.index ?? source.length))) }));
  if (cases.length !== 38 || new Set(cases.map(c => c.id)).size !== 38) throw new Error('Canonical Windows case inventory changed; reconcile the approved scope.');
  return cases;
}

export function verifiedArtifact(root, reference) {
  if (!reference || typeof reference.path !== 'string' || !digestPattern.test(reference.sha256 ?? '') || !reference.role) throw new Error('Evidence requires a role, local relative path and exact SHA256.');
  const path = realpathSync(resolve(root, reference.path)), rel = relative(realpathSync(root), path);
  if (isAbsolute(reference.path) || !rel || rel.startsWith('..') || isAbsolute(rel) || !lstatSync(path).isFile()) throw new Error('Evidence must remain in its declared local bundle.');
  const bytes = readFileSync(path);
  if (/\.(?:key|pem|pfx|p12|env)$/i.test(path)) throw new Error('Private credential/certificate-container files do not belong in qualification evidence.');
  if (bytes.length === 0 || bytes.length > 200 * 1024 * 1024 || sha(bytes) !== reference.sha256) throw new Error('Evidence is missing, changed or exceeds its 200 MiB per-file bound.');
  if (/\.(?:json|txt|log|md)$/i.test(path) && secretViolations(reference.path, bytes.toString('utf8')).length) throw new Error('Credential-shaped bytes found in evidence; redact at its source before recording.');
  return bytes;
}

export function validateCase(record, expected, root) {
  if (record.id !== expected.id || record.canonicalSha256 !== expected.canonicalSha256 || record.status !== 'PASS' || !record.reviewer || !record.observedAt || !record.arrangement || !record.action || !record.observedOutcome || !record.preservationOutcome || !Array.isArray(record.artifacts) || record.artifacts.length < 2) throw new Error(`${expected.id}: pending/stale or insufficient native observations.`);
  for (const role of ['positive-control', 'boundary-control']) if (!record.artifacts.some(item => item.role === role)) throw new Error(`${expected.id}: ${role} evidence is required.`);
  for (const reference of record.artifacts) verifiedArtifact(root, reference);
  if (expected.id === 'TC-WINUI-023') {
    const controls = { newer: ['1.1.0', '1.1.0', 'updated'], same: ['1.0.0', '1.0.0', 'no-update'], older: ['0.9.0', '1.0.0', 'no-update'], malformed: ['not-a-version', '1.0.0', 'failure'], tampered: ['1.1.0', '1.0.0', 'failure'], 'wrong-signature': ['1.1.0', '1.0.0', 'failure'], interrupted: ['1.1.0', '1.0.0', 'failure'] };
    for (const [variant, [offered, installed, outcome]] of Object.entries(controls)) {
      const probe = record.updateVariants?.find(item => item.variant === variant);
      if (!probe || probe.baselineVersion !== '1.0.0' || probe.offeredVersion !== offered || probe.installedVersion !== installed || probe.outcome !== outcome || !probe.usableAfterRelaunch || !digestPattern.test(probe.stateBeforeSha256 ?? '') || probe.stateAfterSha256 !== probe.stateBeforeSha256 || !probe.installedReadback) throw new Error(`TC023: ${variant} needs separate installed-version/state/usability read-back.`);
      verifiedArtifact(root, probe.installedReadback);
    }
  }
}

function readReference(root, reference) { return JSON.parse(verifiedArtifact(root, reference).toString('utf8')); }

export function validateGates(root, gates) {
  if (!Array.isArray(gates) || gates.length !== 25 || new Set(gates.map(item => item.id)).size !== 25) throw new Error('All approved G1–G25 outcomes must be reconciled.');
  for (let n = 1; n <= 25; n++) {
    const gate = gates.find(item => item.id === `G${n}`);
    // G24 applies because the contributor-pin note regenerates local context.
    const required = n <= 21 || n === 24;
    if (!gate || !['PASS', 'N/A'].includes(gate.status) || !gate.rationale || !gate.evidence || (required && gate.status !== 'PASS')) throw new Error(`G${n}: pending or unsubstantiated gate.`);
    verifiedArtifact(root, gate.evidence);
  }
}

export function validateBundle(root, bundle, expectedCases, currentSource) {
  if (bundle.sourceHash !== currentSource || !digestPattern.test(bundle.appSha256 ?? '') || !bundle.build) throw new Error('Qualification bundle source/artifact identity is missing or stale.');
  const build = readReference(root, bundle.build);
  if (build.mode !== 'signed' || build.source?.hash !== currentSource || build.version !== bundle.appVersion || build.target !== 'x86_64-pc-windows-msvc' || !Array.isArray(build.authenticode) || build.authenticode.length !== 2) throw new Error('Signed exact-source app/NSIS build and native signature provenance are required.');
  for (const signature of build.authenticode) if (signature.status !== 'Valid' || !/^[a-fA-F0-9]{40}$/.test(signature.signerThumbprint ?? '') || !signature.timestampThumbprint || !digestPattern.test(signature.sha256 ?? '')) throw new Error('Trusted timestamped app/installer signatures are missing.');
  const assets = bundle.assets ?? [];
  if (!Array.isArray(build.artifacts) || build.artifacts.length !== 3) throw new Error('Exactly app/NSIS/updater-signature build artifacts are required.');
  for (const artifact of build.artifacts) {
    const name = basename(artifact.file.replaceAll('\\', '/'));
    const reference = assets.find(item => item.fileName === name && item.sha256 === artifact.sha256);
    if (!reference) throw new Error(`Missing exact signed build asset: ${name}`);
    verifiedArtifact(root, reference);
  }
  if (!assets.some(item => item.fileName === 'colima-ui.exe' && item.sha256 === bundle.appSha256) || !assets.some(item => item.fileName.endsWith('-setup.exe.sig'))) throw new Error('App and updater signature assets must be present.');
  const identities = new Set();
  for (const name of providers) {
    const provider = bundle.providers?.find(item => item.provider === name);
    if (!provider || !provider.engineId || !provider.context || !provider.providerProvenance) throw new Error(`${name}: required provider/native evidence is pending.`);
    verifiedArtifact(root, provider.providerProvenance);
    const fingerprint = sha(provider.engineId); if (identities.has(fingerprint)) throw new Error('One daemon cannot prove both providers.'); identities.add(fingerprint);
    const boundary = readReference(root, provider.boundary);
    assertBoundaryReceipt(boundary, { provider: name, context: provider.context, 'engine-id': provider.engineId }, boundary.runId);
    if (boundary.source?.hash !== currentSource || boundary.os?.ProductType !== 1 || !/Windows 11/i.test(boundary.os?.Caption ?? '')) throw new Error('Boundary receipt is not current native Windows 11 evidence.');
    for (const count of ['20', '200']) {
      const metrics = readReference(root, provider.metrics?.[count]);
      assertMetricsReceipt(metrics, { provider: name, context: provider.context, 'engine-id': provider.engineId, count }, metrics.runId, bundle.appSha256, bundle.appVersion);
      if (metrics.source?.hash !== currentSource) throw new Error('Metrics source differs from the qualified build.');
    }
    const records = provider.cases ?? [];
    if (records.length !== 37 || new Set(records.map(item => item.id)).size !== 37) throw new Error(`${name}: all native cases except shared preservation TC020 are required.`);
    for (const expected of expectedCases.filter(item => item.id !== 'TC-WINUI-020')) {
      const record = records.find(item => item.id === expected.id); if (!record) throw new Error(`${expected.id}: missing native case.`);
      validateCase(record, expected, root);
    }
    const owner = readReference(root, provider.accounts?.owner), outsider = readReference(root, provider.accounts?.outsider);
    if (!/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(owner.runId ?? '') || owner.runId !== outsider.runId) throw new Error('Owner/outsider account controls must belong to the same arranged run UUID.');
    if (owner.result !== 'PASS' || outsider.result !== 'PASS' || owner.role !== 'owner' || outsider.role !== 'outsider' || owner.accountSid === outsider.accountSid || owner.accountSid !== owner.ownerSid || owner.ownerSid !== outsider.ownerSid || owner.administratorMember !== false || outsider.administratorMember !== false || owner.referencePathSha256 !== outsider.referencePathSha256 || !digestPattern.test(owner.referencePathSha256 ?? '') || owner.port !== outsider.port || owner.privateReadable !== true || outsider.privateReadable !== false || outsider.privateDenial !== 'UnauthorizedAccess' || owner.healthStatus !== 200 || outsider.healthStatus !== 200 || owner.protectedStatus !== 200 || outsider.protectedStatus !== 401) throw new Error('Distinct ordinary owner/outsider private-file and API controls are incomplete.');
  }
  const preservation = expectedCases.find(item => item.id === 'TC-WINUI-020');
  for (const platform of ['macos', 'linux']) {
    const record = bundle.preservation?.find(item => item.platform === platform);
    if (!record) throw new Error(`${platform}: existing-platform preservation evidence pending.`);
    validateCase(record, preservation, root);
  }
  validateGates(root, bundle.gates);
  return build;
}

function pendingCase(item) {
  return { ...item, status: 'PENDING', reviewer: '', observedAt: '', arrangement: '', action: '', observedOutcome: '', preservationOutcome: '', artifacts: [], instruction: 'Read the entire canonical case, including all variants, before executing. Attach distinct positive/boundary artifacts; this worksheet creates no acceptance.' };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = fileURLToPath(new URL('..', import.meta.url)), cases = canonicalCases(readFileSync(resolve(root, 'docs/specs/windows-docker-ui.md'), 'utf8'));
    const [mode, directory, receipt] = process.argv.slice(2), out = resolve(directory ?? '');
    if (!['init', 'verify'].includes(mode) || !directory || (mode === 'init' && !receipt)) throw new Error('Usage: node scripts/windows-evidence.mjs init tmp/<bundle> <build.json> OR verify tmp/<bundle>');
    const tmp = realpathSync(resolve(root, 'tmp')), parent = resolve(out, '..');
    if (!realpathSync(parent).startsWith(`${tmp}/`) && realpathSync(parent) !== tmp && !realpathSync(parent).startsWith(`${tmp}\\`)) throw new Error('Create a fresh bundle inside the checkout tmp tree.');
    if (mode === 'init') {
      mkdirSync(out); mkdirSync(resolve(out, 'artifacts'));
      const build = JSON.parse(readFileSync(receipt, 'utf8')), assets = [];
      for (const artifact of build.artifacts) {
        const name = artifact.file.replaceAll('\\', '/');
        if (!name.startsWith('src-tauri/target/x86_64-pc-windows-msvc/release/') || name.split('/').includes('..')) throw new Error('Build artifacts must come from the declared Windows release output.');
        const fileName = basename(name), path = realpathSync(resolve(root, name));
        const rel = relative(realpathSync(resolve(root, 'src-tauri/target/x86_64-pc-windows-msvc/release')), path);
        if (rel.startsWith('..') || isAbsolute(rel) || assets.some(item => item.fileName === fileName)) throw new Error('Artifact escaped its output or repeats another asset name.');
        if (sha(readFileSync(path)) !== artifact.sha256) throw new Error('Selected build asset differs.');
        copyFileSync(path, resolve(out, 'artifacts', fileName)); assets.push({ role: 'build-asset', fileName, path: `artifacts/${fileName}`, sha256: artifact.sha256 });
      }
      copyFileSync(receipt, resolve(out, 'build.json'));
      const source = sourceIdentity(root);
      writeFileSync(resolve(out, 'bundle.json'), JSON.stringify({ bundleId: randomUUID(), sourceHash: source.hash, appVersion: build.version, appSha256: assets.find(item => item.fileName === 'colima-ui.exe')?.sha256, build: { role: 'build-receipt', path: 'build.json', sha256: sha(readFileSync(receipt)) }, assets,
        providers: providers.map(provider => ({ provider, engineId: '', context: '', providerProvenance: null, boundary: null, metrics: { 20: null, 200: null }, accounts: { owner: null, outsider: null }, cases: cases.filter(item => item.id !== 'TC-WINUI-020').map(pendingCase) })),
        preservation: ['macos', 'linux'].map(platform => ({ ...pendingCase(cases.find(item => item.id === 'TC-WINUI-020')), platform })),
        gates: Array.from({ length: 25 }, (_, i) => ({ id: `G${i + 1}`, status: 'PENDING', rationale: '', evidence: null })), nativeReview: 'PENDING' }, null, 2));
      console.log('Pending worksheet generated; no native check or acceptance claimed.');
    } else {
      nativeHost();
      const bundle = JSON.parse(readFileSync(resolve(out, 'bundle.json'), 'utf8'));
      const build = validateBundle(out, bundle, cases, sourceIdentity(root).hash);
      const app = bundle.assets.find(item => item.fileName === 'colima-ui.exe'), installer = bundle.assets.find(item => item.fileName.endsWith('-setup.exe'));
      const signature = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', resolve(root, 'scripts/windows-signatures.ps1'), '-Application', resolve(out, app.path), '-Installer', resolve(out, installer.path), '-Thumbprint', build.authenticode[0].signerThumbprint], { encoding: 'utf8', timeout: 60_000 });
      if (signature.status !== 0) throw new Error('Current native Authenticode verification failed.');
      writeFileSync(resolve(out, 'structural-validation.json'), JSON.stringify({ structuralValidation: 'PASS', bundleSha256: sha(readFileSync(resolve(out, 'bundle.json'))), sourceHash: bundle.sourceHash, checkedAt: new Date().toISOString(), nativeReview: 'REQUIRED — inspect all canonical case observations/artifacts; structural validation is not behavior acceptance or publication authority' }, null, 2));
      console.log('Artifact bindings/signatures and worksheet structure validated. Native outcome review and release approval remain separate.');
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
