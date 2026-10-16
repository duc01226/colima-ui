import test from 'node:test';
import assert from 'node:assert/strict';
import { npmIssues, cargoIssues, licenseIssues, newIssues, pinnedCargoAuditVersion } from './check-dependencies.mjs';

test('audit pin accepts direct and Cargo-dispatched banners while refusing another version or extra output', () => {
  for (const banner of ['cargo-audit 0.22.2', 'cargo-audit-audit 0.22.2\n']) assert.equal(pinnedCargoAuditVersion(banner, '0.22.2'), true);
  for (const banner of ['cargo-audit 0.22.1', 'cargo-audit-audit 0.23.0', 'cargo 0.22.2', 'cargo-audit 0.22.2 extra', 'cargo-audit 0.22.2\nother output', '0.22.2', '']) assert.equal(pinnedCargoAuditVersion(banner, '0.22.2'), false);
  assert.equal(pinnedCargoAuditVersion('cargo-audit undefined', undefined), false);
});

test('audit failures and incomplete data cannot masquerade as a clean dependency report', () => {
  assert.throws(() => npmIssues({ error: { code: 'offline' } }));
  assert.throws(() => npmIssues({ vulnerabilities: {} }));
  assert.throws(() => cargoIssues({}));
  assert.deepEqual(npmIssues({ vulnerabilities: {}, metadata: { vulnerabilities: { total: 0 } } }), []);
  assert.deepEqual(cargoIssues({ database: {}, vulnerabilities: { list: [] }, warnings: {} }), []);
});
test('known advisories, unmaintained packages and license uncertainty remain individually visible', () => {
  const npm = npmIssues({ vulnerabilities: { example: { range: '<2', via: [{ source: 42 }] } }, metadata: { vulnerabilities: { total: 1 } } });
  assert.deepEqual(npm, ['npm:example:<2:42']);
  const issue = { package: { name: 'example', version: '1.0.0' }, advisory: { id: 'RUSTSEC-fixture' } };
  assert.deepEqual(cargoIssues({ database: {}, vulnerabilities: { list: [issue] }, warnings: { unmaintained: [issue] } }), ['cargo:vulnerability:example@1.0.0:RUSTSEC-fixture', 'cargo:unmaintained:example@1.0.0:RUSTSEC-fixture']);
  assert.deepEqual(licenseIssues([{ name: 'ok', version: '1', license: 'MIT OR Apache-2.0' }, { name: 'unknown', version: '1', license: null }, { name: 'review', version: '2', license: 'AGPL-3.0' }], 'cargo'), ['license:cargo:unknown@1:unknown', 'license:cargo:review@2:AGPL-3.0']);
});
test('legacy ratchet requires an exact, justified, owned exception and blocks new findings', () => {
  const known = 'cargo:vulnerability:old@1:fixture';
  assert.throws(() => newIssues([known], [{ key: known }]));
  const exceptions = [{ key: known, reason: 'Synthetic pre-existing fixture only', owner: 'fixture', reviewedAt: '2026-10-15' }];
  assert.deepEqual(newIssues([known, 'new'], exceptions), ['new']);
  assert.deepEqual(newIssues([known], exceptions), []);
});
