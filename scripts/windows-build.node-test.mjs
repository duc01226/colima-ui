import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { windowsBuildArgs, windowsBuildEnvironment } from './windows-build.mjs';

test('candidate packaging cannot inherit updater signing or claim qualification', () => {
  const args = windowsBuildArgs('candidate', { TAURI_SIGNING_PRIVATE_KEY: 'fixture-only' });
  assert.deepEqual(args.slice(0, 5), ['build', '--target', 'x86_64-pc-windows-msvc', '--bundles', 'nsis']);
  assert.equal(JSON.parse(args[6]).bundle.createUpdaterArtifacts, false);
  assert.deepEqual(args.slice(-2), ['--', '--locked']);
  assert.ok(args.includes('--ci'));
  assert.ok(!args.join(' ').includes('fixture-only'));
  const source = { TAURI_SIGNING_PRIVATE_KEY: 'fixture-only', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'fixture-password', PATH: 'preserved' };
  assert.deepEqual(windowsBuildEnvironment('candidate', source), { PATH: 'preserved' });
  assert.equal(source.TAURI_SIGNING_PRIVATE_KEY, 'fixture-only');
  assert.deepEqual(windowsBuildEnvironment('signed', source), source);
});

test('signed packaging refuses incomplete credentials rather than silently downgrading', () => {
  for (const env of [{}, { TAURI_SIGNING_PRIVATE_KEY: 'fixture-only' }, { WINDOWS_CERTIFICATE_THUMBPRINT: 'A'.repeat(40), WINDOWS_TIMESTAMP_URL: 'http://timestamp.invalid', TAURI_SIGNING_PRIVATE_KEY: 'fixture-only' }]) {
    assert.throws(() => windowsBuildArgs('signed', env), /requires/);
  }
  assert.throws(() => windowsBuildArgs('publish', {}), /candidate or signed/);
  const args = windowsBuildArgs('signed', { WINDOWS_CERTIFICATE_THUMBPRINT: 'A'.repeat(40), WINDOWS_TIMESTAMP_URL: 'https://timestamp.invalid', TAURI_SIGNING_PRIVATE_KEY: 'fixture-only' });
  assert.equal(JSON.parse(args[6]).bundle.createUpdaterArtifacts, true);
  assert.ok(!args.join(' ').includes('fixture-only'));
});

test('Windows overlay keeps per-user NSIS and a recoverable WebView2 prerequisite', () => {
  const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.windows.conf.json', import.meta.url)));
  assert.deepEqual(config.bundle.targets, ['nsis']);
  assert.equal(config.bundle.windows.nsis.installMode, 'currentUser');
  assert.equal(config.bundle.windows.webviewInstallMode.type, 'downloadBootstrapper');
  assert.equal(config.plugins.updater.windows.installMode, 'passive');
  assert.equal(config.plugins.updater.pubkey, undefined);
});
