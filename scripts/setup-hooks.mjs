// npm prepare must work in native Node on Windows, macOS and Linux. The hooks
// themselves use POSIX sh; Git for Windows supplies that shell when running them.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const probe = spawnSync('git', ['rev-parse', '--is-inside-work-tree'], { cwd: root, encoding: 'utf8' });
if (probe.error?.code === 'ENOENT' || !existsSync(join(root, '.git'))) {
  console.log('Git checkout unavailable; hook setup skipped.');
} else if (probe.error || probe.status !== 0) {
  console.error('Could not inspect the Git checkout for hook setup.');
  process.exitCode = 1;
} else {
  const result = spawnSync('git', ['config', 'core.hooksPath', '.githooks'], { cwd: root, stdio: 'inherit' });
  if (result.error || result.status !== 0) {
    console.error('Could not configure the repository hooks.');
    process.exitCode = 1;
  } else {
    console.log('Git hooks installed: .githooks (pre-push lint gate active)');
  }
}
