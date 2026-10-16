import { readFileSync, readdirSync, mkdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { requireToolchain } from './toolchain.mjs';
import { sourceIdentity } from './windows-qualify.mjs';

export function windowsBuildArgs(mode, env = process.env) {
  if (!['candidate', 'signed'].includes(mode)) throw new Error('Choose candidate or signed.');
  const config = { bundle: { createUpdaterArtifacts: mode === 'signed' } };
  if (mode === 'signed') {
    if (!/^[0-9A-Fa-f]{40}$/.test(env.WINDOWS_CERTIFICATE_THUMBPRINT ?? '') ||
        !/^https:\/\/[^\s]+$/.test(env.WINDOWS_TIMESTAMP_URL ?? '') ||
        !env.TAURI_SIGNING_PRIVATE_KEY) {
      throw new Error('Signed build requires an installed Authenticode certificate, HTTPS timestamp URL and existing updater private key.');
    }
    config.bundle.windows = {
      certificateThumbprint: env.WINDOWS_CERTIFICATE_THUMBPRINT,
      timestampUrl: env.WINDOWS_TIMESTAMP_URL,
      tsp: true,
    };
  }
  return ['build', '--target', 'x86_64-pc-windows-msvc', '--bundles', 'nsis', '--config', JSON.stringify(config), '--ci', '--', '--locked'];
}

export function windowsBuildEnvironment(mode, source) {
  const env = { ...source };
  if (mode === 'candidate') {
    delete env.TAURI_SIGNING_PRIVATE_KEY;
    delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
  }
  return env;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('ENVIRONMENT-BLOCKED: build on native Windows x64 with MSVC/SDK/WebView2.');
    if (process.argv.length !== 3) throw new Error('Usage: node scripts/windows-build.mjs <candidate|signed>');
    const root = fileURLToPath(new URL('..', import.meta.url));
    const toolchain = requireToolchain(root), source = sourceIdentity(root);
    const mode = process.argv[2];
    const args = windowsBuildArgs(mode);
    // A candidate ignores ambient release keys and cannot accidentally mint a
    // signed updater. Signed mode never substitutes a newly generated keypair.
    const env = windowsBuildEnvironment(mode, process.env);
    const out = resolve(root, 'tmp/reports/windows-build');
    mkdirSync(out, { recursive: true });
    const receipt = resolve(out, 'build.json');
    rmSync(receipt, { force: true });
    const started = Date.now();
    const result = spawnSync(process.execPath, [resolve(root, 'node_modules/@tauri-apps/cli/tauri.js'), ...args], { cwd: root, env, stdio: 'inherit' });
    if (result.error || result.status !== 0) throw new Error('Windows package build failed; no qualification receipt was produced.');
    const bundle = resolve(root, 'src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis');
    const installers = readdirSync(bundle).filter(name => name.endsWith('-setup.exe') && statSync(resolve(bundle, name)).mtimeMs >= started);
    if (installers.length !== 1) throw new Error('Expected exactly one newly built NSIS installer; refusing stale/ambiguous output.');
    const app = resolve(root, 'src-tauri/target/x86_64-pc-windows-msvc/release/colima-ui.exe');
    const paths = [app, resolve(bundle, installers[0])];
    let authenticode = null;
    if (mode === 'signed') {
      const signature = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', resolve(root, 'scripts/windows-signatures.ps1'), '-Application', app, '-Installer', paths[1], '-Thumbprint', process.env.WINDOWS_CERTIFICATE_THUMBPRINT], { cwd: root, encoding: 'utf8', timeout: 60_000 });
      if (signature.error || signature.status !== 0) throw new Error('Authenticode qualification failed.');
      authenticode = JSON.parse(signature.stdout);
      paths.push(`${paths[1]}.sig`);
    }
    const artifacts = paths.map(path => ({ file: path.slice(root.length).replaceAll('\\', '/'), bytes: statSync(path).size, sha256: createHash('sha256').update(readFileSync(path)).digest('hex') }));
    if (sourceIdentity(root).hash !== source.hash) throw new Error('Source changed during packaging.');
    writeFileSync(receipt, JSON.stringify({ mode, source, toolchain, version: JSON.parse(readFileSync(resolve(root, 'package.json'))).version, target: 'x86_64-pc-windows-msvc', artifacts, authenticode, updaterSignature: mode === 'signed' ? 'generated; installed verification PENDING' : 'not generated', qualification: 'PENDING — native Windows 11/provider/installed lifecycle evidence required' }, null, 2) + '\n');
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
