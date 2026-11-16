// Assemble the npm packages of @ibkit/node: one package per platform with
// its native addon, and the main package that lists them as optional
// dependencies (npm and bun install only the one that matches the host).
//
// Usage: bun scripts/node-packages.ts <addons-dir> <out-dir>
//   <addons-dir>/<platform>/ibkit.node, for the platforms in PLATFORMS.
//   Run `bun run build:ts` in crates/ibkit-node first.
// Then publish every <out-dir>/<platform> directory, and <out-dir>/main last.
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const PLATFORMS: Record<string, { os: string; cpu: string; libc?: string }> = {
  'darwin-arm64': { os: 'darwin', cpu: 'arm64' },
  'darwin-x64': { os: 'darwin', cpu: 'x64' },
  'linux-x64-gnu': { os: 'linux', cpu: 'x64', libc: 'glibc' },
  'linux-arm64-gnu': { os: 'linux', cpu: 'arm64', libc: 'glibc' },
  'win32-x64-msvc': { os: 'win32', cpu: 'x64' },
};

const [addons, out] = process.argv.slice(2).map((path) => resolve(path));
if (!addons || !out) {
  throw new Error('You gave too few arguments. Use: bun scripts/node-packages.ts <addons-dir> <out-dir>.');
}
const root = join(import.meta.dir, '..', 'crates', 'ibkit-node');
const main = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
if (!existsSync(join(root, 'dist', 'index.js'))) {
  throw new Error('crates/ibkit-node/dist is absent. Run `bun run build:ts` in crates/ibkit-node, then run this script again.');
}

const optional: Record<string, string> = {};
for (const [platform, target] of Object.entries(PLATFORMS)) {
  const addon = join(addons, platform, 'ibkit.node');
  if (!existsSync(addon)) {
    console.warn(`skip ${platform}: ${addon} is absent`);
    continue;
  }
  const name = `${main.name}-${platform}`;
  const dir = join(out, platform);
  mkdirSync(dir, { recursive: true });
  cpSync(addon, join(dir, 'ibkit.node'));
  writeFileSync(
    join(dir, 'package.json'),
    `${JSON.stringify(
      {
        name,
        version: main.version,
        description: `The ${platform} native addon of ${main.name}`,
        license: main.license,
        repository: main.repository,
        main: 'ibkit.node',
        files: ['ibkit.node'],
        os: [target.os],
        cpu: [target.cpu],
        ...(target.libc ? { libc: [target.libc] } : {}),
      },
      null,
      2,
    )}\n`,
  );
  optional[name] = main.version;
  console.log(`wrote ${dir}`);
}
if (Object.keys(optional).length === 0) {
  throw new Error(`No addon was found in ${addons}. Put each build at <addons-dir>/<platform>/ibkit.node.`);
}

const dir = join(out, 'main');
mkdirSync(dir, { recursive: true });
cpSync(join(root, 'dist'), join(dir, 'dist'), { recursive: true });
cpSync(join(root, 'README.md'), join(dir, 'README.md'));
const { devDependencies: _dev, scripts: _scripts, ...published } = main;
writeFileSync(
  join(dir, 'package.json'),
  `${JSON.stringify({ ...published, files: ['dist'], optionalDependencies: optional }, null, 2)}\n`,
);
console.log(`wrote ${dir} with ${Object.keys(optional).length} platform packages`);
