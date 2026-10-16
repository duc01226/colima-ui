// Controlled process boundary for Rust tests on Windows, macOS and Linux.
// Node is a pinned contributor/CI prerequisite; the installed app needs none.
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

switch (process.argv[2]) {
  case 'hello': process.stdout.write('hello world'); break;
  case 'lines': process.stdout.write('a\nb\nc\n'); break;
  case 'byte': process.stdout.write('x'); break;
  case 'failure':
    process.stdout.write('partial');
    process.stderr.write('went wrong', () => { process.exitCode = 3; });
    break;
  case 'replacement':
    process.stdout.write('replacement');
    setTimeout(() => {}, 30_000);
    break;
  case 'hold':
    process.stdout.write('started');
    setTimeout(() => {}, 30_000);
    break;
  case 'fork': {
    const child = spawn(process.execPath, [fileURLToPath(import.meta.url), 'leaf'], { stdio: 'inherit' });
    await new Promise((resolve, reject) => { child.once('spawn', resolve); child.once('error', reject); });
    if (process.argv[3]) writeFileSync(process.argv[3], String(child.pid));
    process.stderr.write('x');
    setTimeout(() => {}, 30_000);
    break;
  }
  case 'leaf': setTimeout(() => {}, 30_000); break;
  case 'large': {
    const chunk = Buffer.alloc(1024 * 1024);
    for (let i = 0; i < 256; i++) {
      await new Promise((resolve, reject) => { process.stdout.write(chunk, error => error ? reject(error) : resolve()); });
    }
    break;
  }
  default: throw new Error('Unknown process fixture mode.');
}
