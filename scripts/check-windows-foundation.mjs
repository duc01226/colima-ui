// Narrow Windows boundary/config/secret sensors. Runtime tests and specialist
// review remain separate gates; source text is not proof an operation executed.
import { readFileSync, lstatSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { applicationVersions } from './release-version.mjs';
import { sourceIdentity } from './windows-qualify.mjs';

export function boundaryViolations(file, source) {
  const code = source.replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\//g, '');
  const findings = [];
  if (/src-tauri\/src\/(child_process|private_file|windows_host|windows_pty)\.rs$/.test(file) && /crate::(?:commands|engine_connection|transfer_registry|services|adapters)|\b(?:container_id|image_ref|docker)\b/i.test(code)) findings.push('generic-native-owner-imports-consumer-policy');
  if (file === 'src-tauri/src/commands/metrics_collector.rs' && /crate::(?:pro|subscription)\b/.test(code)) findings.push('collector-depends-on-paid-build');
  if (file.startsWith('src-tauri/src/') && file !== 'src-tauri/src/engine_connection.rs' && /static\s+SESSION\s*:/.test(code)) findings.push('duplicated-engine-session-owner');
  return findings.map(rule => ({ file, rule }));
}

export function secretViolations(file, source) {
  const rules = {
    'private-key': /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/g,
    'github-token': /\b(?:ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{60,})\b/g,
    'provider-secret': /\b(?:sk-(?:proj-)?[A-Za-z0-9_-]{32,}|sk-ant-[A-Za-z0-9_-]{32,}|xox[baprs]-[A-Za-z0-9-]{24,})\b/g,
    'app-bearer': /\bcolima-[a-f0-9]{64}\b/g,
  };
  const findings = [];
  for (const [rule, pattern] of Object.entries(rules)) for (const match of source.matchAll(pattern)) {
    findings.push({ file, rule, fingerprint: createHash('sha256').update(`${file}\0${rule}\0${match[0]}`).digest('hex') });
  }
  return findings;
}

export function configViolations({ versions, node, pkg, base, windows, toolchain, rust, workflows }) {
  const errors = [];
  if (new Set(versions).size !== 1) errors.push('Application versions differ across npm/Tauri/Cargo/locks.');
  if (!/^\d+\.\d+\.\d+$/.test(node) || !/^npm@\d+\.\d+\.\d+$/.test(pkg.packageManager ?? '')) errors.push('Exact contributor Node/npm pins are required.');
  if (!/^\d+\.\d+\.\d+$/.test(rust)) errors.push('Rust pin must be exact.');
  if (windows.bundle?.targets?.join(',') !== 'nsis' || windows.bundle?.windows?.nsis?.installMode !== 'currentUser' || windows.bundle?.windows?.webviewInstallMode?.type !== 'downloadBootstrapper' || windows.plugins?.updater?.windows?.installMode !== 'passive') errors.push('Windows per-user NSIS/WebView2/updater policy drift.');
  if (typeof base.plugins?.updater?.pubkey !== 'string' || base.plugins.updater.pubkey.length < 20 || windows.plugins?.updater?.pubkey) errors.push('Keep the existing shared updater verification key; no Windows override.');
  for (const [file, text] of Object.entries(workflows)) {
    const code = text.split('\n').filter(line => !line.trim().startsWith('#')).join('\n');
    if (!code.includes(`dtolnay/rust-toolchain@${rust}`) || !code.includes('node-version-file: .node-version')) errors.push(`${file}: toolchain pin drift.`);
    if (file.includes('windows-') || file.endsWith('/ci.yml')) for (const [key, expected] of [['sdk', toolchain.windowsSdk], ['toolset', toolchain.msvcToolset], ['vsversion', toolchain.visualStudio]]) {
      if (!new RegExp(`${key}: ['"]?${expected.replaceAll('.', '\\.')}['"]?(?:\\s|$)`).test(code)) errors.push(`${file}: selected Windows ${key} drift.`);
    }
    if (file.includes('windows-') && /contents:\s*write|tauri-action|create-release|upload-release/.test(code)) errors.push(`${file}: candidate/qualification workflow acquired publication authority.`);
  }
  return errors;
}

export function formatRust(root, paths, check) {
  if (!Array.isArray(paths) || !paths.length) throw new Error('Missing declared Windows Rust formatting scope.');
  const args = ['--edition', '2021', '--config', 'skip_children=true', ...(check ? ['--check'] : []), ...paths];
  const result = spawnSync('rustfmt', args, { cwd: root, encoding: 'utf8', timeout: 60_000, maxBuffer: 16 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`Touched Rust formatting failed; run node scripts/check-windows-foundation.mjs --format.\n${result.stdout ?? ''}${result.stderr ?? ''}`);
}

export function checkFoundation(root) {
  const json = file => JSON.parse(readFileSync(resolve(root, file), 'utf8'));
  const text = file => readFileSync(resolve(root, file), 'utf8');
  const baseline = json('scripts/windows-quality-baseline.json');
  const workflows = Object.fromEntries(['.github/workflows/ci.yml', '.github/workflows/release.yml', '.github/workflows/windows-candidate.yml', '.github/workflows/windows-qualification.yml'].map(file => [file, text(file)]));
  const errors = configViolations({ versions: applicationVersions(root), node: text('.node-version').trim(), pkg: json('package.json'), base: json('src-tauri/tauri.conf.json'), windows: json('src-tauri/tauri.windows.conf.json'), toolchain: json('scripts/windows-toolchain.json'), rust: text('rust-toolchain.toml').match(/^channel = "([^"]+)"/m)?.[1], workflows });
  for (const [file] of sourceIdentity(root).files) {
    if (!/\.(?:rs|[cm]?js|ts|svelte|json|ya?ml|toml|ps1|sh)$/.test(file)) continue;
    if (lstatSync(resolve(root, file)).isSymbolicLink()) { errors.push(`${file}: refusing to scan code through a symlink`); continue; }
    const source = text(file);
    for (const finding of boundaryViolations(file, source)) errors.push(`${finding.file}: ${finding.rule}`);
    for (const finding of secretViolations(file, source)) if (!baseline.secretFingerprints.includes(finding.fingerprint)) errors.push(`${finding.file}: ${finding.rule} (value redacted, fingerprint ${finding.fingerprint})`);
  }
  try { formatRust(root, baseline.rustFormatPaths, true); } catch (error) { errors.push(error.message); }
  return errors;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length > 3 || (process.argv[2] && process.argv[2] !== '--format')) throw new Error('Usage: node scripts/check-windows-foundation.mjs [--format]');
    const root = fileURLToPath(new URL('..', import.meta.url));
    if (process.argv[2] === '--format') {
      formatRust(root, JSON.parse(readFileSync(resolve(root, 'scripts/windows-quality-baseline.json'))).rustFormatPaths, false);
      console.log('Declared Windows Rust scope formatted; no quality gate or test was run.');
    } else {
      const errors = checkFoundation(root);
      if (errors.length) throw new Error(errors.join('\n'));
      console.log('Windows foundation config/boundary/secret/scoped-format checks passed. Native runtime qualification is separate.');
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
