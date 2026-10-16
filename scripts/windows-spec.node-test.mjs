import test from 'node:test';
import assert from 'node:assert/strict';
import { qualificationIntent } from './windows-spec.mjs';
test('recording proof preserves intent identity while a changed control/outcome invalidates it', () => {
  const source = '---\nstatus: draft\n---\n# Rule\nKeep original bytes.\n\n**Evidence:** authored\n**CoveredBy:** Untested\n**Status:** Planned\n';
  const accepted = source.replace('draft', 'accepted').replace('authored', 'real native receipt').replace('Untested', 'native result').replace('Planned', 'Verified');
  assert.equal(qualificationIntent(source), qualificationIntent(accepted));
  assert.notEqual(qualificationIntent(source), qualificationIntent(source.replace('Keep original bytes.', 'Replace original bytes.')));
  assert.notEqual(qualificationIntent(source), qualificationIntent(source + '\nNegative control: reject replacement engine.'));
  assert.equal(qualificationIntent(source), qualificationIntent(source.replaceAll('\n', '\r\n')));
  const policy = source.replace('status: draft', 'status: draft\nqualification_policy:\n  terminal_cap: 16');
  assert.notEqual(qualificationIntent(policy), qualificationIntent(policy.replace('terminal_cap: 16', 'terminal_cap: 32')));
});
