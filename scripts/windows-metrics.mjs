// TC021 installed-app tier. Run interactively on Windows 11; the observer never
// opens an activating metrics subscription or installs/manages an engine.
import { createHash, randomUUID } from 'node:crypto';
import { readFileSync, writeFileSync, realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { qualificationOptions, qualificationEnvironment, nativeHost, sourceIdentity } from './windows-qualify.mjs';
import { requireToolchain } from './toolchain.mjs';

export function metricsOptions(args) {
  const specific = {}, common = [];
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i]?.slice(2);
    if (['app', 'build-receipt', 'count'].includes(key)) {
      if (specific[key] || !args[i + 1] || args[i + 1].startsWith('--')) throw new Error('Use each metrics option once with a value.');
      specific[key] = args[i + 1];
    } else common.push(...args.slice(i, i + 2));
  }
  if (!specific.app || !specific['build-receipt'] || !['20', '200'].includes(specific.count)) throw new Error('Required: --app installed.exe --build-receipt build.json --count 20|200 plus dedicated provider options.');
  return { ...qualificationOptions(common), ...specific };
}

export function assertMetricsReceipt(receipt, options, run, appHash, version) {
  const fingerprint = createHash('sha256').update(options['engine-id']).digest('hex');
  if (receipt.result !== 'PASS' || receipt.runId !== run || receipt.providerDeclared !== options.provider || receipt.context !== options.context || receipt.engineFingerprint !== fingerprint || receipt.appSha256 !== appHash || receipt.appVersion !== version || receipt.workloads !== Number(options.count) || receipt.intervalMs !== 2000 || !Number.isInteger(receipt.processId) || receipt.processId <= 0 || receipt.idleBeforeAndAfter !== true || receipt.ownedCleanup !== true) throw new Error('Missing matching current installed-app metrics assertions.');
  for (const name of ['normal', 'recovered']) {
    const window = receipt[name];
    if (!window || !Number.isFinite(window.cpuSeconds) || window.cpuSeconds < 0 || !Number.isFinite(window.wallSeconds) || window.wallSeconds < 60 || !Array.isArray(window.ticks) || window.ticks.length < 29) throw new Error('Missing full metrics window.');
    const limit = Number(options.count) === 20 ? 2 : 5;
    if (window.cpuSeconds / window.wallSeconds * 100 >= limit) throw new Error('CPU budget exceeded.');
    const durations = window.ticks.map((tick, index) => {
      if (!Number.isFinite(tick.durationMs) || tick.durationMs < 0 || tick.failed !== false || tick.samples !== Number(options.count) || !Number.isSafeInteger(tick.sequence) || tick.sequence < 1 || (index > 0 && tick.sequence !== window.ticks[index - 1].sequence + 1)) throw new Error('Incomplete/failed fleet tick.');
      return tick.durationMs;
    }).sort((a, b) => a - b);
    if (durations[Math.ceil(durations.length * 0.95) - 1] >= 2000) throw new Error('Tick budget exceeded.');
  }
  if (!Array.isArray(receipt.points) || receipt.points.length > 1800 || receipt.points.some((point, index) => !Number.isFinite(point.elapsedMs) || point.elapsedMs < 0 || !Number.isFinite(point.residentBytes) || point.residentBytes <= 0 || !Number.isFinite(point.cpuSeconds) || point.cpuSeconds < 0 || !Number.isInteger(point.liveProcesses) || point.liveProcesses <= 0 || !Number.isInteger(point.totalProcesses) || point.totalProcesses < point.liveProcesses || (index > 0 && (point.elapsedMs <= receipt.points[index - 1].elapsedMs || point.elapsedMs - receipt.points[index - 1].elapsedMs > 1500 || point.cpuSeconds < receipt.points[index - 1].cpuSeconds)))) throw new Error('Missing required process sample or cadence.');
  const baseline = receipt.points.filter(point => point.elapsedMs >= receipt.baselineStartMs && point.elapsedMs < receipt.firstFleetCreationMs);
  const loaded = receipt.points.filter(point => point.elapsedMs >= receipt.firstFleetCreationMs && point.elapsedMs <= receipt.peakEndMs);
  if (baseline.length < 60 || receipt.firstFleetCreationMs - receipt.baselineStartMs < 60_000 || !loaded.length) throw new Error('Missing same-run pre-fleet baseline or loaded peak.');
  const max = points => Math.max(...points.map(point => point.residentBytes));
  const base = max(baseline), peak = max(loaded), growth = Math.max(0, peak - base);
  if (receipt.baselineResidentBytes !== base || receipt.peakResidentBytes !== peak || receipt.residentGrowthBytes !== growth || (Number(options.count) === 200 && growth >= 268435456)) throw new Error('Resident budget or baseline/peak arithmetic failed.');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = metricsOptions(process.argv.slice(2));
    const os = nativeHost();
    const root = fileURLToPath(new URL('..', import.meta.url)); const toolchain = requireToolchain(root);
    const source = sourceIdentity(root);
    const app = realpathSync(options.app), appHash = createHash('sha256').update(readFileSync(app)).digest('hex');
    const build = JSON.parse(readFileSync(options['build-receipt'], 'utf8'));
    if (build.source?.hash !== source.hash || !['candidate', 'signed'].includes(build.mode) || build.target !== 'x86_64-pc-windows-msvc' || !build.artifacts?.some(artifact => artifact.file.endsWith('/release/colima-ui.exe') && artifact.sha256 === appHash) || build.version !== JSON.parse(readFileSync(resolve(root, 'package.json'))).version) throw new Error('Installed app must match the selected current release build receipt.');
    const run = randomUUID(), env = qualificationEnvironment(options, run);
    Object.assign(env, { COLIMAUI_TEST_APP_PATH: app, COLIMAUI_TEST_APP_SHA256: appHash, COLIMAUI_TEST_APP_VERSION: build.version, COLIMAUI_TEST_METRICS_COUNT: options.count });
    const result = spawnSync('cargo', ['test', '--locked', '--release', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', '--', '--exact', 'windows_metrics_cost::installed_monitoring_cost', '--ignored', '--nocapture', '--test-threads=1'], { cwd: root, env, stdio: 'inherit', timeout: 30 * 60_000 });
    if (result.error || result.status !== 0) throw new Error('Installed-app metrics qualification failed; no success inferred from missing measurements.');
    const path = resolve(root, 'tmp/reports/windows-native', run, 'metrics.json');
    const receipt = JSON.parse(readFileSync(path, 'utf8')); assertMetricsReceipt(receipt, options, run, appHash, build.version);
    if (sourceIdentity(root).hash !== source.hash) throw new Error('Source changed during measurement.');
    writeFileSync(path, `${JSON.stringify({ ...receipt, os, source, build, toolchain, node: process.versions.node }, null, 2)}\n`);
    console.log(`Installed metrics assertions passed: tmp/reports/windows-native/${run}/metrics.json. Complete release qualification remains pending.`);
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
