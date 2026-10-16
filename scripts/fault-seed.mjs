// Bounded defect-seeding fallback. Use after the final reviewed tree passes its
// affected suite. Backups and receipts live in tmp; no Git restoration occurs.
import { createHash, randomUUID } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, realpathSync, existsSync, unlinkSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { stripVTControlCharacters } from 'node:util';
import { sourceIdentity } from './windows-qualify.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
function within(root, file) {
  const path = realpathSync(resolve(root, file)), rel = relative(realpathSync(root), path);
  if (!rel || rel.startsWith('..') || isAbsolute(rel) || rel.split(/[\\/]/).some(part => ['.git', 'node_modules', 'target', 'tmp', 'temp'].includes(part))) throw new Error('Mutations require an existing source file inside the selected root.');
  return path;
}

export function prepareSeed(root, seed) {
  if (!/^[a-z0-9-]+$/.test(seed.id ?? '') || !seed.intent || !seed.case || typeof seed.from !== 'string' || !seed.from || typeof seed.to !== 'string' || seed.from === seed.to || !['node', 'cargo'].includes(seed.command) || !Array.isArray(seed.args) || seed.args.some(arg => typeof arg !== 'string' || arg.includes('\0')) || !seed.args.length) throw new Error('Each seed requires an identity, case, protected intent, exact defect and literal Node/Cargo test argv.');
  const path = within(root, seed.file), original = readFileSync(path), needle = Buffer.from(seed.from);
  const at = original.indexOf(needle);
  if (at < 0 || original.indexOf(needle, at + 1) >= 0) throw new Error(`Seed ${seed.id} anchor is missing/ambiguous; update its source mapping before running.`);
  const mutated = Buffer.concat([original.subarray(0, at), Buffer.from(seed.to), original.subarray(at + needle.length)]);
  return { path, original, mutated };
}

export function runCommand(root, command, args, timeoutMs, log) {
  return new Promise((done, reject) => {
    const child = spawn(command === 'node' ? process.execPath : command, args, { cwd: root, shell: false, detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'] });
    let bytes = 0, output = '', timedOut = false, overflow = false, spawnError, interrupted = false;
    const kill = () => {
      if (!child.pid) return;
      if (process.platform === 'win32') spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', timeout: 10_000 });
      else { try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') spawnError = error; } }
    };
    const timer = setTimeout(() => { timedOut = true; kill(); }, timeoutMs);
    const interrupt = () => { interrupted = true; kill(); };
    process.on('SIGINT', interrupt); process.on('SIGTERM', interrupt);
    const consume = chunk => {
      bytes += chunk.length;
      if (bytes > 8 * 1024 * 1024) { overflow = true; kill(); return; }
      output += chunk.toString('utf8');
    };
    child.stdout.on('data', consume); child.stderr.on('data', consume);
    child.on('error', error => { spawnError = error; });
    child.on('close', (code, signal) => {
      clearTimeout(timer);
      process.off('SIGINT', interrupt); process.off('SIGTERM', interrupt);
      // A Unix helper must not survive the test command's process group.
      if (process.platform !== 'win32') kill();
      try {
        writeFileSync(log, output);
        done({ code, signal, timedOut, overflow, interrupted, spawnError: spawnError?.code ?? null, output });
      } catch (error) { reject(error); }
    });
  });
}

export async function runSeed(root, seed, directory, timeoutMs = 120_000) {
  const { path, original, mutated } = prepareSeed(root, seed);
  mkdirSync(directory, { recursive: true });
  const backup = resolve(directory, 'source.backup'), statePath = resolve(directory, 'state.json');
  writeFileSync(backup, original, { flag: 'wx' });
  const state = { id: seed.id, file: seed.file, case: seed.case, intent: seed.intent, command: [seed.command, ...seed.args], originalSha256: sha(original), mutatedSha256: sha(mutated), status: 'PREPARED', restored: false };
  const save = () => writeFileSync(statePath, `${JSON.stringify(state, null, 2)}\n`);
  save();
  // Keep restoration conflicts authoritative even when test execution fails.
  function restoreOriginal() {
    const current = readFileSync(path), saved = readFileSync(backup);
    if (sha(saved) !== state.originalSha256) { state.status = 'BACKUP-CONFLICT'; save(); throw new Error('Backup changed; refusing unverified restoration.'); }
    if (!current.equals(mutated) && !current.equals(original)) { state.status = 'SOURCE-CONFLICT'; save(); throw new Error('Source changed concurrently; backup retained, source not overwritten.'); }
    writeFileSync(path, saved);
    state.restored = readFileSync(path).equals(saved) && saved.equals(original);
    save();
    if (!state.restored) throw new Error('Exact-byte restoration failed; stop before any further check.');
  }
  let result;
  try {
    writeFileSync(path, mutated); state.status = 'MUTATED'; save();
    result = await runCommand(root, seed.command, seed.args, timeoutMs, resolve(directory, 'test.log'));
    const text = stripVTControlCharacters(result.output);
    state.status = result.timedOut ? 'TIMEOUT' : result.interrupted ? 'INTERRUPTED' : result.overflow || result.spawnError || result.signal ? 'INVALID' : result.code === 0 ? 'SURVIVED' : /test result: FAILED|AssertionError|Tests\s+\d+\s+failed/.test(text) ? 'KILLED' : 'INVALID';
    state.exitCode = result.code; state.signal = result.signal; state.log = 'test.log';
  } finally {
    restoreOriginal();
  }
  return state;
}

export function recoverSeed(root, directory) {
  const statePath = resolve(directory, 'state.json'), state = JSON.parse(readFileSync(statePath, 'utf8'));
  const path = within(root, state.file), saved = readFileSync(resolve(directory, 'source.backup')), current = readFileSync(path);
  if (sha(saved) !== state.originalSha256 || ![state.originalSha256, state.mutatedSha256].includes(sha(current))) throw new Error('Recovery conflict: source or backup differs; refusing overwrite.');
  writeFileSync(path, saved);
  if (!readFileSync(path).equals(saved)) throw new Error('Recovery byte comparison failed.');
  writeFileSync(statePath, `${JSON.stringify({ ...state, status: 'INTERRUPTED-RESTORED', restored: true }, null, 2)}\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('..', import.meta.url)), lock = resolve(root, 'tmp/fault-seed/active.json');
  let acquired = false;
  try {
    mkdirSync(dirname(lock), { recursive: true });
    if (process.argv[2] === '--recover' && process.argv.length === 3) {
      const pending = JSON.parse(readFileSync(lock, 'utf8'));
      if (!/^[a-f0-9-]{36}$/.test(pending.run) || !/^[a-z0-9-]+$/.test(pending.seed)) throw new Error('Invalid recovery identity.');
      if (!Number.isInteger(pending.pid) || pending.pid <= 0) throw new Error('Missing prior process ownership; inspect recovery manually.');
      try { process.kill(pending.pid, 0); throw new Error('Prior mutation process is still active; recovery refused.'); }
      catch (error) { if (error.code !== 'ESRCH') throw error; }
      recoverSeed(root, resolve(root, 'tmp/fault-seed', pending.run, pending.seed)); unlinkSync(lock);
      console.log('Interrupted source restored exactly; rerun the affected green baseline before mutation.');
    } else {
      if (process.argv.length !== 4) throw new Error('Usage: node scripts/fault-seed.mjs <manifest.json> <final-green-baseline.json> OR --recover');
      if (existsSync(lock)) throw new Error('A prior run is active/interrupted; inspect its state before --recover.');
      const source = sourceIdentity(root), baseline = JSON.parse(readFileSync(process.argv[3], 'utf8'));
      if (baseline.result !== 'PASS' || baseline.source?.hash !== source.hash) throw new Error('Final green baseline is missing/stale; mutation is verify-last only.');
      const seeds = JSON.parse(readFileSync(process.argv[2], 'utf8')).seeds;
      if (!Array.isArray(seeds) || seeds.length === 0 || seeds.length > 100 || new Set(seeds.map(s => s.id)).size !== seeds.length) throw new Error('Use 1–100 unique reviewed seeds per bounded run.');
      for (const seed of seeds) {
        prepareSeed(root, seed);
        if (seed.platforms && !seed.platforms.includes(process.platform)) throw new Error(`ENVIRONMENT-BLOCKED: seed ${seed.id} requires ${seed.platforms.join(',')}. Select the native manifest; never count a skipped seed.`);
        if (seed.command === 'cargo' && (!seed.args.includes('--lib') || !seed.args.includes('test') || seed.args.includes('--ignored'))) throw new Error('Fault seeds use non-destructive library tests, never ignored live engine cases.');
        if (seed.command === 'node' && !['node_modules/vitest/vitest.mjs', '--test'].includes(seed.args[0])) throw new Error('Fault seeds use the existing Vitest/Node test runner.');
      }
      const run = randomUUID(), results = [];
      for (const seed of seeds) {
        writeFileSync(lock, JSON.stringify({ run, seed: seed.id, pid: process.pid }), { flag: acquired ? 'w' : 'wx' }); acquired = true;
        const result = await runSeed(root, seed, resolve(root, 'tmp/fault-seed', run, seed.id)); results.push(result);
        writeFileSync(resolve(root, 'tmp/fault-seed', run, 'ledger.json'), JSON.stringify({ source, results, result: results.length === seeds.length && results.every(r => r.status === 'KILLED' && r.restored) ? 'PASS' : 'INCOMPLETE' }, null, 2));
        if (result.status !== 'KILLED') throw new Error(`Seed ${seed.id}: ${result.status}; inspect its named test log before proceeding.`);
      }
      if (sourceIdentity(root).hash !== source.hash) throw new Error('Source differs after restoration; no green mutation proof.');
      console.log(`Killed ${results.length}/${seeds.length}; exact source restored. Ledger: tmp/fault-seed/${run}/ledger.json`);
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
  finally {
    if (acquired && existsSync(lock)) {
      const pending = JSON.parse(readFileSync(lock, 'utf8'));
      const statePath = resolve(root, 'tmp/fault-seed', pending.run, pending.seed, 'state.json');
      if (existsSync(statePath) && JSON.parse(readFileSync(statePath, 'utf8')).restored) unlinkSync(lock);
    }
  }
}
