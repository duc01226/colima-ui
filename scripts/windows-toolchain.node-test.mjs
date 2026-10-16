import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, copyFileSync, mkdirSync, mkdtempSync, rmSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { validateWindowsToolchain } from './toolchain.mjs';
const policy = JSON.parse(readFileSync(new URL('./windows-toolchain.json', import.meta.url)));
const env = { WindowsSDKVersion: `${policy.windowsSdk}\\`, VCToolsVersion: `${policy.msvcToolset}.12345`, VSCMD_VER: `${policy.visualStudioMajor}.14.1`, VSCMD_ARG_TGT_ARCH: 'x64', VCToolsInstallDir: 'fixture-vc', WindowsSdkDir: 'fixture-sdk' };
// A caller's Git overrides must never redirect fixture writes to its checkout.
const fixtureEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_')));
test('Windows build provenance requires the selected SDK, servicing family and native x64 developer environment', () => {
  assert.equal(validateWindowsToolchain(policy, env).sdk, policy.windowsSdk);
  for (const change of [{ WindowsSDKVersion: '10.0.1.0' }, { VCToolsVersion: '14.10.12345' }, { VSCMD_ARG_TGT_ARCH: 'arm64' }, { VSCMD_VER: '' }, { WindowsSdkDir: '' }]) assert.throws(() => validateWindowsToolchain(policy, { ...env, ...change }));
});

// TechnicalSpec=WINUI-portable-hook-bootstrap: real Node/Git boundary in an
// isolated fixture only. This is not native Windows hook-shell qualification.
test('prepare configures only its own checkout regardless of the launch directory', () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'hook-bootstrap-'));
  try {
    const checkout = resolve(root, 'checkout 空間'), elsewhere = resolve(root, 'elsewhere');
    mkdirSync(resolve(checkout, 'scripts'), { recursive: true }); mkdirSync(elsewhere);
    const script = resolve(checkout, 'scripts/setup-hooks.mjs');
    copyFileSync(new URL('./setup-hooks.mjs', import.meta.url), script);
    const initialized = spawnSync('git', ['init', '--quiet', checkout], { env: fixtureEnv, encoding: 'utf8', timeout: 10_000 });
    assert.equal(initialized.status, 0, initialized.stderr);
    const result = spawnSync(process.execPath, [script], { cwd: elsewhere, env: fixtureEnv, encoding: 'utf8', timeout: 10_000 });
    assert.equal(result.status, 0, result.stderr);
    const selected = spawnSync('git', ['-C', checkout, 'config', '--local', '--get', 'core.hooksPath'], { env: fixtureEnv, encoding: 'utf8', timeout: 10_000 });
    assert.equal(selected.status, 0, selected.stderr);
    assert.equal(selected.stdout.trim(), '.githooks');
    assert.equal(existsSync(resolve(elsewhere, '.git')), false);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

// TechnicalSpec=WINUI-portable-hook-bootstrap: package archives may have no
// Git metadata; a present broken checkout must instead fail with a useful error.
test('prepare distinguishes an unpacked archive from invalid checkout metadata', () => {
  const parent = resolve('tmp/tooling-tests'); mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(resolve(parent, 'hook-unpacked-'));
  try {
    mkdirSync(resolve(root, 'scripts'));
    const script = resolve(root, 'scripts/setup-hooks.mjs');
    copyFileSync(new URL('./setup-hooks.mjs', import.meta.url), script);
    const unpacked = spawnSync(process.execPath, [script], { cwd: root, env: fixtureEnv, encoding: 'utf8', timeout: 10_000 });
    assert.equal(unpacked.status, 0, unpacked.stderr);
    assert.match(unpacked.stdout, /Git checkout unavailable/);
    writeFileSync(resolve(root, '.git'), 'gitdir: missing-fixture-metadata\n');
    const broken = spawnSync(process.execPath, [script], { cwd: root, env: fixtureEnv, encoding: 'utf8', timeout: 10_000 });
    assert.equal(broken.status, 1, broken.stderr);
    assert.match(broken.stderr, /Could not inspect the Git checkout/);
    assert.equal(readFileSync(resolve(root, '.git'), 'utf8'), 'gitdir: missing-fixture-metadata\n');
  } finally { rmSync(root, { recursive: true, force: true }); }
});
