// Native app-library boundary tier. Installed UI/lifecycle/performance evidence
// is separate; a successful run here never declares Windows release readiness.
import { createHash, randomUUID } from 'node:crypto';
import { readFileSync, writeFileSync, lstatSync, readlinkSync, realpathSync } from 'node:fs';
import { machine } from 'node:os';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { requireToolchain } from './toolchain.mjs';
import { qualificationIntent } from './windows-spec.mjs';
import { versionUpdates } from './release-version.mjs';

export function updateFixtureInput(file, bytes) {
  if (file !== 'src-tauri/tauri.conf.json') return bytes;
  const config = JSON.parse(bytes.toString('utf8'));
  if (config.plugins?.updater?.endpoints) config.plugins.updater.endpoints = ['controlled-qualification-feed'];
  return JSON.stringify(config);
}

export function qualificationOptions(args) {
  const values = {};
  const allowed = new Set(['provider', 'context', 'engine-id', 'fixture-image', 'fixture-id', 'dedicated-engine', 'profile']);
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i]?.replace(/^--/, '');
    if (!args[i]?.startsWith('--') || !allowed.has(key) || values[key] || !args[i + 1] || args[i + 1].startsWith('--')) throw new Error('Use each required --name value once.');
    values[key] = args[i + 1];
  }
  if ([...allowed].some(key => !values[key])) throw new Error('Required: --provider --context --engine-id --fixture-image --fixture-id --dedicated-engine --profile');
  if (!['docker-desktop', 'rancher-moby'].includes(values.provider)) throw new Error('Choose docker-desktop or rancher-moby.');
  if (!/^[\p{L}\p{N}._-]+$/u.test(values.context) || values.context.startsWith('-') || Buffer.byteLength(values.context) > 255) throw new Error('Invalid context.');
  if (!/^sha256:[a-fA-F0-9]{64}$/.test(values['fixture-id'])) throw new Error('Provide the exact local fixture image content ID.');
  if (values['dedicated-engine'] !== values['engine-id'] || values.profile !== 'disposable-account') throw new Error('Explicitly name the dedicated engine ID and disposable-account profile.');
  return values;
}

export function assertBoundaryReceipt(receipt, options, run) {
  const fingerprint = createHash('sha256').update(options['engine-id']).digest('hex');
  if (receipt.result !== 'PASS' || receipt.runId !== run || receipt.providerDeclared !== options.provider || receipt.context !== options.context || receipt.engineFingerprint !== fingerprint) {
    throw new Error('Missing/current-run engine-bound application assertions; zero tests or a stale receipt cannot pass.');
  }
  for (const boundary of ['containers', 'images', 'volumes', 'networks', 'copy hash/TAR roundtrip', 'image export/import', 'Compose Unicode file', 'ConPTY UTF8/resize/interrupt/exit', 'terminal external ownership', 'metrics attribution', 'owned cleanup']) {
    if (!receipt.asserted?.includes(boundary)) throw new Error(`Required boundary missing: ${boundary}`);
  }
}

export function sourceIdentity(root) {
  const files = spawnSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', 'src', 'src-tauri', 'scripts', 'tests', '.github', '.cargo', '.githooks', '.gitattributes', 'README.md', 'external_skill', 'package.json', 'package-lock.json', '.node-version', 'rust-toolchain.toml', 'vite.config.*', 'vitest.config.*', 'svelte.config.*', 'eslint.config.*', 'tsconfig*.json', 'index.html', 'docs/specs/windows-docker-ui.md'], { cwd: root, encoding: 'utf8' });
  if (files.status !== 0) throw new Error('Cannot identify the source tree for native evidence.');
  const normalizedVersions = versionUpdates(root, '0.0.0-fixture');
  const fixtureEntries = [];
  const entries = [...new Set(files.stdout.split('\0').filter(Boolean))].sort().map(file => {
    const path = resolve(root, file);
    const bytes = lstatSync(path).isSymbolicLink() ? Buffer.from(readlinkSync(path)) : readFileSync(realpathSync(path));
    const identityBytes = file === 'docs/specs/windows-docker-ui.md' ? qualificationIntent(bytes.toString('utf8')) : bytes;
    const fixtureBytes = normalizedVersions.get(path) ?? identityBytes;
    fixtureEntries.push([file, createHash('sha256').update(updateFixtureInput(file, fixtureBytes)).digest('hex')]);
    return [file, createHash('sha256').update(identityBytes).digest('hex')];
  });
  if (!entries.length) throw new Error('Empty source identity.');
  return { hash: createHash('sha256').update(JSON.stringify(entries)).digest('hex'), files: entries,
    updateFixtureHash: createHash('sha256').update(JSON.stringify(fixtureEntries)).digest('hex'),
    specRawSha256: createHash('sha256').update(readFileSync(resolve(root, 'docs/specs/windows-docker-ui.md'))).digest('hex'),
    specIdentity: 'documented single-line evidence carriers and root bookkeeping metadata excluded; qualification policies and canonical intent retained' };
}

export function nativeHost() {
  if (process.platform !== 'win32' || process.arch !== 'x64' || machine() !== 'x86_64') throw new Error('ENVIRONMENT-BLOCKED: native Windows 11 x64 required.');
  const host = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', 'Get-CimInstance Win32_OperatingSystem | Select-Object Caption,BuildNumber,ProductType,Version | ConvertTo-Json -Compress'], { encoding: 'utf8', timeout: 30_000 });
  if (host.status !== 0) throw new Error('ENVIRONMENT-BLOCKED: cannot verify Windows host.');
  const os = JSON.parse(host.stdout);
  if (os.ProductType !== 1 || Number(os.BuildNumber) < 22000 || !/Windows 11/i.test(os.Caption)) throw new Error('ENVIRONMENT-BLOCKED: this is not the supported Windows 11 workstation target.');
  return os;
}

export function qualificationEnvironment(options, run) {
  const env = { ...process.env,
    COLIMAUI_TEST_RUN_ID: run, COLIMAUI_TEST_PROVIDER: options.provider,
    COLIMAUI_TEST_CONTEXT: options.context, COLIMAUI_TEST_ENGINE_ID: options['engine-id'],
    COLIMAUI_TEST_IMAGE: options['fixture-image'], COLIMAUI_TEST_IMAGE_ID: options['fixture-id'],
    COLIMAUI_TEST_DESTRUCTIVE: `dedicated:${options['dedicated-engine']}`, COLIMAUI_TEST_PROFILE: options.profile,
    DOCKER_CONTEXT: options.context,
  };
  for (const key of ['DOCKER_HOST', 'DOCKER_TLS', 'DOCKER_TLS_VERIFY', 'DOCKER_CERT_PATH', 'DOCKER_API_VERSION']) delete env[key];
  return env;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = qualificationOptions(process.argv.slice(2));
    const os = nativeHost();
    const root = fileURLToPath(new URL('..', import.meta.url));
    const toolchain = requireToolchain(root);
    const source = sourceIdentity(root);
    const run = randomUUID();
    const env = qualificationEnvironment(options, run);
    // Selection is process-local input, not `docker context use`. Existing saved
    // app preference must agree; the suite refuses a different selection.
    const result = spawnSync('cargo', ['test', '--locked', '--release', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', '--', '--exact', 'windows_provider_contract::native::native_provider_core_contract', '--ignored', '--nocapture', '--test-threads=1'], { cwd: root, env, stdio: 'inherit', timeout: 30 * 60_000 });
    if (result.error || result.status !== 0) throw new Error('Native application-boundary assertions failed; qualification remains pending.');
    const path = resolve(root, 'tmp/reports/windows-native', run, 'boundary.json');
    const receipt = JSON.parse(readFileSync(path, 'utf8'));
    assertBoundaryReceipt(receipt, options, run);
    if (sourceIdentity(root).hash !== source.hash) throw new Error('Source changed during qualification; evidence is stale.');
    writeFileSync(path, `${JSON.stringify({ ...receipt, os, source, toolchain, node: process.versions.node, rust: 'rust-toolchain.toml' }, null, 2)}\n`);
    console.log(`Native app-library assertions passed: tmp/reports/windows-native/${run}/boundary.json. Complete installed qualification remains pending.`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
