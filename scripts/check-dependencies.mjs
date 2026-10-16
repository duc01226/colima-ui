// Networked contributor check: npm registry audit and RustSec advisory/index
// fetches receive public dependency names/versions. No app credentials are used.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { npmEntry, requireToolchain } from './toolchain.mjs';

export function pinnedCargoAuditVersion(output, expected) {
  if (!/^\d+\.\d+\.\d+$/.test(expected)) return false;
  // The pinned CLI reports its subcommand name when dispatched by Cargo.
  // Both presentations must still match the complete, exact policy version.
  return [`cargo-audit ${expected}`, `cargo-audit-audit ${expected}`].includes(output.trim());
}

export function npmIssues(report) {
  if (report.error || !report.vulnerabilities || typeof report.vulnerabilities !== 'object' || !report.metadata?.vulnerabilities) throw new Error('ENVIRONMENT-BLOCKED: npm audit data is unavailable/malformed.');
  return Object.entries(report.vulnerabilities).flatMap(([name, item]) => {
    if (!Array.isArray(item.via) || typeof item.range !== 'string') throw new Error('Malformed npm vulnerability evidence.');
    return item.via.map(via => `npm:${name}:${item.range}:${typeof via === 'string' ? via : via.source ?? via.url ?? via.title}`);
  });
}

export function cargoIssues(report) {
  if (!report.database || !Array.isArray(report.vulnerabilities?.list) || typeof report.warnings !== 'object' || report.warnings === null) throw new Error('ENVIRONMENT-BLOCKED: Cargo audit data is unavailable/malformed.');
  const key = (kind, issue) => {
    if (!issue.package?.name || !issue.package?.version) throw new Error('Malformed Cargo advisory package evidence.');
    return `cargo:${kind}:${issue.package.name}@${issue.package.version}:${issue.advisory?.id ?? kind}`;
  };
  const issues = report.vulnerabilities.list.map(issue => key('vulnerability', issue));
  for (const [kind, items] of Object.entries(report.warnings)) {
    if (!Array.isArray(items)) throw new Error('Malformed Cargo warning evidence.');
    issues.push(...items.map(issue => key(kind, issue)));
  }
  return issues;
}

export function licenseIssues(packages, ecosystem) {
  return packages.filter(pkg => !pkg.license || /\b(?:AGPL|GPL)-/.test(pkg.license))
    .map(pkg => `license:${ecosystem}:${pkg.name}@${pkg.version}:${pkg.license ?? 'unknown'}`);
}

export function newIssues(issues, accepted) {
  if (!Array.isArray(accepted) || accepted.some(item => typeof item.key !== 'string' || !item.reason || !item.owner || !item.reviewedAt)) throw new Error('Legacy dependency exceptions require exact keys, rationale, owner and review date.');
  const allowed = new Set(accepted.map(item => item.key));
  return [...new Set(issues)].filter(key => !allowed.has(key)).sort();
}

function run(command, args, root) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', timeout: 180_000, maxBuffer: 32 * 1024 * 1024 });
  if (result.error || result.signal || result.stdout.trim() === '') throw new Error(`ENVIRONMENT-BLOCKED: ${command} dependency check did not return complete evidence.`);
  return result;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = fileURLToPath(new URL('..', import.meta.url)), toolchain = requireToolchain(root);
    const policy = JSON.parse(readFileSync(resolve(root, 'scripts/windows-toolchain.json')));
    const auditVersion = spawnSync('cargo', ['audit', '--version'], { cwd: root, encoding: 'utf8', timeout: 30_000 });
    if (auditVersion.status !== 0 || !pinnedCargoAuditVersion(auditVersion.stdout, policy.cargoAudit)) throw new Error(`ENVIRONMENT-BLOCKED: install cargo-audit with cargo install cargo-audit --version ${policy.cargoAudit} --locked.`);
    const out = resolve(root, 'tmp/reports/windows-quality'); mkdirSync(out, { recursive: true });
    const npm = run(process.execPath, [npmEntry(), 'audit', '--json'], root);
    const cargo = run('cargo', ['audit', '--file', 'src-tauri/Cargo.lock', '--json'], root);
    const metadata = run('cargo', ['metadata', '--locked', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml'], root);
    if (![0, 1].includes(cargo.status) || metadata.status !== 0) throw new Error('ENVIRONMENT-BLOCKED: Cargo audit/index/metadata did not complete successfully.');
    writeFileSync(resolve(out, 'npm-audit.json'), npm.stdout); writeFileSync(resolve(out, 'cargo-audit.json'), cargo.stdout);
    const npmReport = JSON.parse(npm.stdout), cargoReport = JSON.parse(cargo.stdout), packages = JSON.parse(metadata.stdout).packages;
    const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json')));
    const npmPackages = Object.entries(lock.packages).filter(([path]) => path).map(([path, pkg]) => ({ name: pkg.name ?? path.split('node_modules/').at(-1), version: pkg.version, license: pkg.license }));
    const issues = [...npmIssues(npmReport), ...cargoIssues(cargoReport), ...licenseIssues(packages, 'cargo'), ...licenseIssues(npmPackages, 'npm')];
    const baseline = JSON.parse(readFileSync(resolve(root, 'scripts/windows-quality-baseline.json')));
    const findings = newIssues(issues, baseline.dependencyExceptions);
    writeFileSync(resolve(out, 'dependency-health.json'), JSON.stringify({ toolchain, issues: [...new Set(issues)].sort(), newIssues: findings, acceptedLegacy: baseline.dependencyExceptions, result: findings.length ? 'FAIL' : 'PASS', note: 'Scoped fail-on-new result; not a declaration of zero legacy risk.' }, null, 2));
    if (findings.length) throw new Error(`New/unreviewed dependency or license findings:\n${findings.join('\n')}`);
    console.log('Scoped dependency-health ratchet passed; raw reports and explicit legacy debt are in tmp/reports/windows-quality.');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
