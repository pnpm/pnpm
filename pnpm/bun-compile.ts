/**
 * Compiles pnpm into a single executable using Bun.
 *
 * Prerequisites:
 *   1. Run `pnpm run _compile` (tsgo --build) to compile TypeScript sources to lib/
 *
 * Usage: bun run bun-compile.ts
 *
 * This produces a standalone binary at `./pnpm-bun` that includes
 * the Bun runtime and all pnpm code.
 */
import { $ } from 'bun'
import fs from 'fs'
import path from 'path'

const outFile = 'pnpm-bun'

async function main (): Promise<void> {
  // Verify prerequisites
  if (!fs.existsSync('lib/pnpm.js')) {
    console.error('lib/pnpm.js not found. Run `pnpm run _compile` first (tsgo --build).')
    process.exit(1)
  }
  if (!fs.existsSync('../worker/lib/worker.js')) {
    console.error('../worker/lib/worker.js not found. Run `pnpm run _compile` first.')
    process.exit(1)
  }

  // Step 1: Bundle the worker separately so we can embed its source.
  // The worker runs in a worker_thread and must exist as a standalone file.
  console.log('Bundling worker...')
  const workerResult = await Bun.build({
    entrypoints: ['../worker/lib/worker.js'],
    target: 'bun',
    format: 'esm',
  })
  if (!workerResult.success) {
    console.error('Worker bundle failed:')
    for (const log of workerResult.logs) console.error(log)
    process.exit(1)
  }
  const workerCode = await workerResult.outputs[0].text()
  // Prepend the require shim to the worker bundle too, in case it needs it
  const workerWithShim = `import { createRequire as _cr } from 'module';globalThis.require = globalThis.require || _cr(import.meta.url);\n${workerCode}`

  // Step 2: Collect native addon files for extraction
  const nativeAddonCode = generateNativeAddonExtraction()

  // Step 3: Create entry point.
  // Uses globalThis.require instead of const require so that eval('require')
  // in @yarnpkg/fslib works correctly inside compiled Bun binaries.
  const entrySource = `
import { createRequire } from 'module';
import fs from 'fs';
import os from 'os';
import path from 'path';

// Shim require on globalThis so eval('require') in @yarnpkg/fslib works.
// bun build --compile scopes const/let variables, but globalThis persists.
globalThis.require = createRequire(import.meta.url);

process.setMaxListeners(0);
globalThis['pnpm__startedAt'] = Date.now();

// Extract the embedded worker.js to a temp location so worker_threads can load it.
const _workerCode = ${JSON.stringify(workerWithShim)};
const _tmpDir = path.join(os.tmpdir(), 'pnpm-bun-' + process.pid);
fs.mkdirSync(_tmpDir, { recursive: true });
const _workerPath = path.join(_tmpDir, 'worker.js');
fs.writeFileSync(_workerPath, _workerCode);
${nativeAddonCode}
process.env.PNPM_WORKER_PATH = _workerPath;

// Cleanup temp files on exit
process.on('exit', () => {
  try { fs.rmSync(_tmpDir, { recursive: true, force: true }); } catch {}
});

// Import pnpm CLI from compiled TypeScript lib sources.
// Bun will bundle all workspace dependencies.
await import('./lib/pnpm.js');
`

  const entryFile = '_bun_entry.mjs'
  fs.writeFileSync(entryFile, entrySource)

  // Step 4: Compile to single executable
  console.log('Compiling pnpm single executable with Bun...')
  try {
    await $`bun build --compile ${entryFile} --outfile ${outFile}`
    console.log(`\nSuccess! Binary created: ./${outFile}`)
    console.log(`Size: ${(fs.statSync(outFile).size / 1024 / 1024).toFixed(1)} MB`)
    console.log(`\nTest it with: ./${outFile} --version`)
  } catch (err) {
    console.error('Compilation failed:', err)
    process.exit(1)
  } finally {
    try { fs.unlinkSync(entryFile) } catch {}
  }
}

function generateNativeAddonExtraction (): string {
  if (!fs.existsSync('dist')) return ''
  const nodeFiles = fs.readdirSync('dist').filter(f => f.endsWith('.node'))
  if (nodeFiles.length === 0) return ''

  const lines: string[] = []
  for (const nodeFile of nodeFiles) {
    const content = fs.readFileSync(path.join('dist', nodeFile))
    const b64 = content.toString('base64')
    lines.push(`fs.writeFileSync(path.join(_tmpDir, ${JSON.stringify(nodeFile)}), Buffer.from(${JSON.stringify(b64)}, 'base64'));`)
  }
  return '\n' + lines.join('\n')
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
