import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, isAbsolute, join } from 'node:path';
import { spawnSync } from 'node:child_process';

export function validateWindowsToolchain(policy, env) {
  const sdk = env.WindowsSDKVersion?.replace(/[\\/]+$/, '');
  if (sdk !== policy.windowsSdk || !env.VCToolsVersion?.startsWith(`${policy.msvcToolset}.`) || Number(env.VSCMD_VER?.split('.')[0]) !== policy.visualStudioMajor || !['x64', 'amd64'].includes(env.VSCMD_ARG_TGT_ARCH) || !env.VCToolsInstallDir || !env.WindowsSdkDir) {
    throw new Error(`Use Visual Studio ${policy.visualStudio} x64 Developer PowerShell with MSVC ${policy.msvcToolset} servicing tools and Windows SDK ${policy.windowsSdk}.`);
  }
  return { sdk, msvc: env.VCToolsVersion, developerEnvironment: env.VSCMD_VER };
}

function windowsToolchain(root) {
  const policy = JSON.parse(readFileSync(join(root, 'scripts/windows-toolchain.json'), 'utf8'));
  const versions = validateWindowsToolchain(policy, process.env);
  const compiler = realpathSync(join(process.env.VCToolsInstallDir, 'bin/Hostx64/x64/cl.exe'));
  const linker = realpathSync(join(process.env.VCToolsInstallDir, 'bin/Hostx64/x64/link.exe'));
  if (!existsSync(join(process.env.WindowsSdkDir, 'Include', versions.sdk, 'um/Windows.h'))) throw new Error('Selected Windows SDK headers are missing.');
  const first = spawnSync('where.exe', ['cl.exe'], { encoding: 'utf8', timeout: 10_000 });
  if (first.status !== 0 || realpathSync(first.stdout.trim().split(/\r?\n/)[0]).toLowerCase() !== compiler.toLowerCase()) throw new Error('Compiler PATH does not match the selected x64 toolset.');
  const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
  return { ...versions, compilerSha256: hash(compiler), linkerSha256: hash(linker) };
}

export function npmEntry() {
  const bin = dirname(process.execPath);
  const candidates = [process.env.npm_execpath, join(bin, 'node_modules/npm/bin/npm-cli.js'), join(bin, '../lib/node_modules/npm/bin/npm-cli.js')];
  const npmCli = candidates.find(p => p && isAbsolute(p) && existsSync(p) && p.endsWith('npm-cli.js'));
  if (!npmCli) throw new Error('Run through npm so its pinned JS entry can be located.');
  return npmCli;
}

export function requireToolchain(root) {
  const node = readFileSync(join(root, '.node-version'), 'utf8').trim();
  const rust = readFileSync(join(root, 'rust-toolchain.toml'), 'utf8').match(/^channel = "([^"]+)"/m)?.[1];
  const npm = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).packageManager?.replace(/^npm@/, '');
  if (process.versions.node !== node) throw new Error(`Use Node ${node} from .node-version.`);
  for (const command of ['rustc', 'cargo']) {
    const result = spawnSync(command, ['--version'], { encoding: 'utf8', timeout: 30_000 });
    if (result.status !== 0 || !result.stdout.startsWith(`${command} ${rust} `)) {
      throw new Error(`Use Rust ${rust} with MSVC prerequisites on Windows (rust-toolchain.toml).`);
    }
  }
  // Execute npm's JS entry with Node. Windows .cmd files need a shell, which
  // would change quoting; neither this runner nor the build needs that fallback.
  const npmCli = npmEntry();
  const result = spawnSync(process.execPath, [npmCli, '--version'], { encoding: 'utf8', timeout: 30_000 });
  if (result.status !== 0 || result.stdout.trim() !== npm) throw new Error(`Use npm ${npm} (packageManager).`);
  return { node, npm, rust, ...(process.platform === 'win32' ? { windows: windowsToolchain(root) } : {}) };
}
