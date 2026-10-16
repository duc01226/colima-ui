import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { boundaryViolations, secretViolations, configViolations } from './check-windows-foundation.mjs';
const json = file => JSON.parse(readFileSync(new URL(`../${file}`, import.meta.url)));
function example() {
  const policy = json('scripts/windows-toolchain.json');
  return { versions: Array(6).fill('1.2.3-rc.1'), node: '24.21.0', pkg: { packageManager: 'npm@11.19.0' }, base: json('src-tauri/tauri.conf.json'), windows: json('src-tauri/tauri.windows.conf.json'), toolchain: policy, rust: '1.99.0', workflows: { '.github/workflows/windows-candidate.yml': `uses: dtolnay/rust-toolchain@1.99.0\nnode-version-file: .node-version\nsdk: '${policy.windowsSdk}'\ntoolset: '${policy.msvcToolset}'\nvsversion: '${policy.visualStudio}'\npermissions:\n  contents: read\n` } };
}
test('generic native owners cannot acquire consumer policy; collector stays independent from paid builds', () => {
  const file = 'src-tauri/src/child_process.rs';
  assert.deepEqual(boundaryViolations(file, '// crate::commands::containers is not an import\nuse std::process::Command;'), []);
  assert.equal(boundaryViolations(file, 'use crate::commands::containers;').length, 1);
  assert.equal(boundaryViolations(file, 'struct State { container_id: String }').length, 1);
  assert.equal(boundaryViolations('src-tauri/src/commands/metrics_collector.rs', 'use crate::pro::Writer;').length, 1);
  assert.equal(boundaryViolations('src-tauri/src/commands/runtime.rs', 'static SESSION: u8 = 0;').length, 1);
  assert.deepEqual(boundaryViolations('src-tauri/src/engine_connection.rs', 'static SESSION: u8 = 0;'), []);
});
test('secret detector scans test and production files but emits only rule and stable fingerprints', () => {
  for (const value of ['ghp_' + 'a'.repeat(36), 'sk-proj-' + 'a'.repeat(40), 'colima-' + 'a'.repeat(64), '-----BEGIN ' + 'PRIVATE KEY-----']) {
    const findings = secretViolations('src/fixture.test.ts', `const value = '${value}';`);
    assert.ok(findings.length > 0);
    assert.ok(findings.every(finding => /^[a-f0-9]{64}$/.test(finding.fingerprint)));
    assert.ok(!JSON.stringify(findings).includes(value));
  }
  assert.deepEqual(secretViolations('src/config.ts', 'sha256:' + 'a'.repeat(64)), []);
});
test('Windows delivery and complete version identity cannot drift or acquire publication', () => {
  assert.deepEqual(configViolations(example()), []);
  const changes = [
    d => { d.versions[5] = '1.2.3'; }, d => { d.pkg.packageManager = 'npm@latest'; },
    d => { d.windows.bundle.windows.nsis.installMode = 'perMachine'; },
    d => { d.windows.plugins.updater.pubkey = 'replacement'; },
    d => { d.windows.bundle.windows.webviewInstallMode.type = 'skip'; },
    d => { d.workflows['.github/workflows/windows-candidate.yml'] += 'contents: write\n'; },
    d => { d.workflows['.github/workflows/windows-candidate.yml'] = 'node-version: latest'; },
  ];
  for (const change of changes) { const data = example(); change(data); assert.ok(configViolations(data).length > 0); }
});
