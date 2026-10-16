// Controlled HTTPS loopback feed for TC023. Never generates signing keys,
// executes installers, changes an engine, or publishes artifacts.
import { createServer } from 'node:https';
import { createReadStream, readFileSync, statSync, mkdirSync, appendFileSync, writeFileSync } from 'node:fs';
import { createHash, randomUUID } from 'node:crypto';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { sourceIdentity } from './windows-qualify.mjs';

export const UPDATE_VARIANTS = ['newer', 'same', 'older', 'malformed', 'tampered', 'wrong-signature', 'interrupted'];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

export async function signedFixture(receiptPath, version, family) {
  const build = JSON.parse(readFileSync(receiptPath, 'utf8'));
  if (build.version !== version || build.mode !== 'signed' || build.target !== 'x86_64-pc-windows-msvc' || build.source?.updateFixtureHash !== family || !Array.isArray(build.authenticode) || build.authenticode.length !== 2 || build.authenticode.some(item => item.status !== 'Valid' || !item.timestampThumbprint)) throw new Error('Use signed Windows builds from the prepared same-source/version-only fixtures.');
  const workspace = resolve(dirname(receiptPath), '../../..');
  const installer = build.artifacts?.find(item => item.file.endsWith('-setup.exe'));
  const signature = build.artifacts?.find(item => item.file === `${installer?.file}.sig`);
  if (!installer || !signature) throw new Error('Missing original NSIS/updater-signature assets.');
  const file = item => {
    const name = item.file.replaceAll('\\', '/');
    if (!name.startsWith('src-tauri/target/x86_64-pc-windows-msvc/release/') || name.split('/').includes('..')) throw new Error('Fixture asset escaped its declared Windows output.');
    return resolve(workspace, name);
  };
  const path = file(installer), size = statSync(path).size;
  if (size < 2 || size > 200 * 1024 * 1024) throw new Error('Unexpected fixture installer size.');
  const sha = createHash('sha256'); for await (const chunk of createReadStream(path)) sha.update(chunk);
  if (sha.digest('hex') !== installer.sha256) throw new Error('Fixture installer hash changed.');
  const signatureBytes = readFileSync(file(signature));
  if (!signatureBytes.length || signatureBytes.length > 8192 || hash(signatureBytes) !== signature.sha256) throw new Error('Fixture signature hash/size changed.');
  return { version, path, size, signature: signatureBytes.toString('utf8').trim(), installerSha256: installer.sha256 };
}

export function updateOffer(variant, fixtures, origin) {
  if (!UPDATE_VARIANTS.includes(variant)) throw new Error('Unknown controlled update variant.');
  const selected = variant === 'same' ? fixtures.baseline : variant === 'older' ? fixtures.older : fixtures.newer;
  const signature = variant === 'wrong-signature' ? fixtures.baseline.signature : selected.signature;
  const entry = { signature, url: `${origin}/installer.exe` };
  return { version: variant === 'malformed' ? 'not-a-version' : selected.version, notes: `Controlled qualification ${variant}; not a release feed.`, pub_date: '2026-10-01T00:00:00Z', platforms: { 'windows-x86_64': entry, 'windows-x86_64-nsis': entry } };
}

export function updateHandler(variant, fixtures, origin, log = () => {}) {
  updateOffer(variant, fixtures, origin);
  return async (request, response) => {
    try {
      const path = new URL(request.url, origin).pathname;
      if (request.method !== 'GET') { response.writeHead(405); response.end(); return; }
      if (path === '/update') {
        response.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
        response.end(JSON.stringify(updateOffer(variant, fixtures, origin)));
        log({ variant, path, deliveryOnly: true }); return;
      }
      if (path !== '/installer.exe') { response.writeHead(404); response.end(); return; }
      const selected = variant === 'same' ? fixtures.baseline : variant === 'older' ? fixtures.older : fixtures.newer;
      response.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': selected.size, 'cache-control': 'no-store' });
      const interrupt = variant === 'interrupted';
      const stream = createReadStream(selected.path, interrupt ? { start: 0, end: Math.floor(selected.size / 2) - 1 } : {});
      log({ variant, path, declaredBytes: selected.size, interrupted: interrupt, tampered: variant === 'tampered', deliveryOnly: true });
      if (interrupt) {
        stream.once('error', () => response.destroy());
        stream.once('end', () => response.destroy());
        response.once('close', () => stream.destroy());
        stream.pipe(response, { end: false });
      } else {
        let sent = 0;
        const change = new Transform({ transform(chunk, _encoding, callback) {
          if (variant === 'tampered' && sent + chunk.length === selected.size) chunk[chunk.length - 1] ^= 1;
          sent += chunk.length; callback(null, chunk);
        } });
        await pipeline(stream, change, response);
      }
    } catch { response.destroy(); log({ variant, error: 'controlled-feed-delivery-failed', deliveryOnly: true }); }
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = {};
    for (let i = 2; i < process.argv.length; i += 2) {
      const key = process.argv[i]?.slice(2);
      if (!process.argv[i]?.startsWith('--') || !['variant', 'newer', 'baseline', 'older', 'tls-cert', 'tls-key', 'port'].includes(key) || options[key] || !process.argv[i + 1]) throw new Error('Use each --variant --newer --baseline --older --tls-cert --tls-key --port option once.');
      options[key] = process.argv[i + 1];
    }
    if (Object.keys(options).length !== 7 || !UPDATE_VARIANTS.includes(options.variant) || !/^\d+$/.test(options.port) || Number(options.port) < 1024 || Number(options.port) > 65535) throw new Error('Missing/invalid controlled feed inputs.');
    const root = fileURLToPath(new URL('..', import.meta.url)), source = sourceIdentity(root);
    const fixtures = { newer: await signedFixture(resolve(options.newer), '1.1.0', source.updateFixtureHash), baseline: await signedFixture(resolve(options.baseline), '1.0.0', source.updateFixtureHash), older: await signedFixture(resolve(options.older), '0.9.0', source.updateFixtureHash) };
    if (fixtures.newer.installerSha256 === fixtures.baseline.installerSha256 || fixtures.newer.signature === fixtures.baseline.signature) throw new Error('Newer and baseline must be distinct real signed artifacts.');
    const out = resolve(root, 'tmp/reports/windows-update-feed', randomUUID()); mkdirSync(out, { recursive: true });
    const logPath = resolve(out, `${options.variant}.jsonl`); writeFileSync(logPath, '', { flag: 'wx' });
    let records = 0;
    const log = record => { if (++records > 1000) throw new Error('Controlled request limit exceeded.'); appendFileSync(logPath, `${JSON.stringify({ at: new Date().toISOString(), ...record })}\n`); };
    const server = createServer({ cert: readFileSync(options['tls-cert']), key: readFileSync(options['tls-key']), handshakeTimeout: 10_000 }, updateHandler(options.variant, fixtures, `https://127.0.0.1:${options.port}`, log));
    server.maxConnections = 8; server.requestTimeout = 30_000; server.headersTimeout = 10_000;
    server.on('error', () => { console.error('Controlled HTTPS feed failed; no updater outcome is accepted.'); process.exitCode = 1; });
    server.listen(Number(options.port), '127.0.0.1', () => console.log(`Controlled ${options.variant} feed on HTTPS loopback. Delivery log: ${logPath}. Installed-version/state read-back is still required.`));
    const close = () => { server.close(); server.closeAllConnections(); };
    process.once('SIGINT', close); process.once('SIGTERM', close);
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
