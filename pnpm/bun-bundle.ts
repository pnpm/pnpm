/**
 * Bundles pnpm using Bun's bundler API.
 * This is the Bun equivalent of bundle.ts (which uses esbuild).
 *
 * Usage: bun run bun-bundle.ts
 */
import { $ } from 'bun'
import fs from 'fs'
import path from 'path'

const banner = `import { createRequire as _cr } from 'module';const require = _cr(import.meta.url); const __filename = import.meta.filename; const __dirname = import.meta.dirname`

async function main (): Promise<void> {
  // Bundle the main pnpm CLI
  const mainResult = await Bun.build({
    entrypoints: ['lib/pnpm.js'],
    outdir: 'dist',
    naming: 'pnpm.mjs',
    target: 'node',
    format: 'esm',
    sourcemap: 'external',
    banner,
    define: {
      'process.env.npm_package_name': JSON.stringify(
        process.env.npm_package_name ?? 'pnpm'
      ),
      'process.env.npm_package_version': JSON.stringify(
        process.env.npm_package_version ?? ''
      ),
    },
    external: [
      'node-gyp',
      './get-uid-gid.js',
    ],
  })

  if (!mainResult.success) {
    console.error('Main bundle failed:')
    for (const log of mainResult.logs) {
      console.error(log)
    }
    process.exit(1)
  }
  console.log('Bundled dist/pnpm.mjs')

  // Bundle the worker
  const workerResult = await Bun.build({
    entrypoints: ['../worker/lib/worker.js'],
    outdir: 'dist',
    naming: 'worker.js',
    target: 'node',
    format: 'esm',
    sourcemap: 'external',
    banner,
  })

  if (!workerResult.success) {
    console.error('Worker bundle failed:')
    for (const log of workerResult.logs) {
      console.error(log)
    }
    process.exit(1)
  }
  console.log('Bundled dist/worker.js')

  // Copy static assets (same as the esbuild bundle.ts post-build steps)
  const copies: Array<{ src: string, dest: string }> = [
    { src: 'node-gyp-bin', dest: 'dist/node-gyp-bin' },
    { src: 'node_modules/@pnpm/tabtab/lib/templates', dest: 'dist/templates' },
    { src: 'node_modules/ps-list/vendor', dest: 'dist/vendor' },
  ]

  for (const { src, dest } of copies) {
    if (fs.existsSync(src)) {
      fs.cpSync(src, dest, { recursive: true })
      console.log(`Copied ${src} -> ${dest}`)
    } else {
      console.warn(`Warning: ${src} not found, skipping copy`)
    }
  }

  // Copy pnpmrc
  if (fs.existsSync('pnpmrc')) {
    fs.copyFileSync('pnpmrc', 'dist/pnpmrc')
    console.log('Copied pnpmrc -> dist/pnpmrc')
  }

  // Copy any .node native addon files from the esbuild output
  // Bun's bundler doesn't have a .node loader like esbuild, so we need
  // to find and copy them manually
  await copyNativeAddons()
}

async function copyNativeAddons (): Promise<void> {
  // Find .node files in node_modules that pnpm depends on
  const glob = new Bun.Glob('node_modules/**/*.node')
  for await (const file of glob.scan('.')) {
    const dest = path.join('dist', path.basename(file))
    fs.copyFileSync(file, dest)
    console.log(`Copied native addon: ${file} -> ${dest}`)
  }
  // Also check parent workspace node_modules
  const parentGlob = new Bun.Glob('../node_modules/**/*.node')
  for await (const file of parentGlob.scan('.')) {
    const dest = path.join('dist', path.basename(file))
    if (!fs.existsSync(dest)) {
      fs.copyFileSync(file, dest)
      console.log(`Copied native addon: ${file} -> ${dest}`)
    }
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
