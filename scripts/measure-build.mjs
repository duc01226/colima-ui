// F6: record actual warm/no-op build behavior on the final reviewed tree.
// Native bundle/signing/runtime qualification is not implied by these builds.
import { writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { sourceIdentity } from './windows-qualify.mjs';
import { npmEntry, requireToolchain } from './toolchain.mjs';

export function cargoArtifacts(stdout) {
  const messages = stdout.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  const finished = messages.find(message => message.reason === 'build-finished');
  const artifacts = messages.filter(message => message.reason === 'compiler-artifact');
  if (!finished?.success || artifacts.length === 0 || artifacts.some(item => typeof item.fresh !== 'boolean')) throw new Error('Missing successful Cargo artifact/cache evidence.');
  return { count: artifacts.length, fresh: artifacts.filter(item => item.fresh).length, rebuilt: artifacts.filter(item => !item.fresh).map(item => item.package_id) };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 2) throw new Error('Usage: node scripts/measure-build.mjs');
    const root = fileURLToPath(new URL('..', import.meta.url)), toolchain = requireToolchain(root), source = sourceIdentity(root);
    const out = resolve(root, 'tmp/reports/windows-build-cost'); mkdirSync(out, { recursive: true });
    const records = [];
    for (const pass of ['warm', 'noop']) {
      for (const tier of ['frontend', 'rust-library']) {
        const command = tier === 'frontend' ? process.execPath : 'cargo';
        const args = tier === 'frontend' ? [npmEntry(), 'run', 'build'] : ['build', '--locked', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', '--all-features', '--message-format=json'];
        const start = performance.now();
        const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', timeout: 10 * 60_000, maxBuffer: 64 * 1024 * 1024 });
        writeFileSync(resolve(out, `${pass}-${tier}.log`), `${result.stdout ?? ''}\n${result.stderr ?? ''}`);
        if (result.error || result.status !== 0) throw new Error(`${pass} ${tier} build failed; preserve logs before judging cache behavior.`);
        const cache = tier === 'rust-library' ? cargoArtifacts(result.stdout) : { note: 'Vite performs a complete bundle per invocation; record time, do not claim artifact caching.' };
        records.push({ pass, tier, elapsedMs: performance.now() - start, cache });
        if (pass === 'noop' && tier === 'rust-library' && cache.rebuilt.length) throw new Error('Unchanged Rust build recompiled artifacts; investigate its cache/rebuild reason.');
      }
    }
    if (sourceIdentity(root).hash !== source.hash) throw new Error('Source changed during build measurement.');
    writeFileSync(resolve(out, 'build-cost.json'), JSON.stringify({ result: 'PASS', platform: process.platform, arch: process.arch, toolchain, source, records, nativeQualification: 'PENDING' }, null, 2));
    console.log('Warm/no-op build observations saved in tmp/reports/windows-build-cost/build-cost.json.');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
