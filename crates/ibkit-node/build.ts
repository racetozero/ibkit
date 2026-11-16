// Build the native addon with Cargo and copy it to ./ibkit.node.
// Usage: bun build.ts [--debug]
import { copyFileSync } from 'node:fs';
import { join } from 'node:path';

const debug = process.argv.includes('--debug');
const cargo = Bun.spawnSync(['cargo', 'build', ...(debug ? [] : ['--release'])], {
  cwd: import.meta.dir,
  stdout: 'inherit',
  stderr: 'inherit',
});
if (cargo.exitCode !== 0) {
  throw new Error('cargo build failed; read the compiler output above and correct the error.');
}
const file = { darwin: 'libibkit_node.dylib', win32: 'ibkit_node.dll' }[process.platform as string] ?? 'libibkit_node.so';
copyFileSync(join(import.meta.dir, 'target', debug ? 'debug' : 'release', file), join(import.meta.dir, 'ibkit.node'));
console.log(`wrote ibkit.node (${debug ? 'debug' : 'release'})`);
