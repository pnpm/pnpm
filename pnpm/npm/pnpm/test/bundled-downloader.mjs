// The entry point loads get-pnpm from the wrapper's `dist/node_modules`, where
// scripts/bundle-node-gyp.mjs puts it and its dependencies for the release. A
// fixture that runs the entry point stands in for that.
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** Copies get-pnpm's published files and its dependencies into `wrapperDir`'s `dist/node_modules`. */
export function bundleDownloader (wrapperDir) {
  for (const [name, packageDir] of downloaderPackages()) {
    const dest = path.join(wrapperDir, 'dist/node_modules', name)
    if (name === 'get-pnpm') {
      copyPublishedFiles(packageDir, dest)
    } else {
      fs.cpSync(packageDir, dest, { dereference: true, recursive: true })
    }
  }
}

/**
 * The workspace package's own `files`, which is what the release's
 * `pnpm deploy` injects: its build output, not its sources or dev dependencies.
 */
function copyPublishedFiles (packageDir, dest) {
  if (!fs.existsSync(path.join(packageDir, 'lib', 'index.js'))) {
    throw new Error(`get-pnpm is not built in ${packageDir}. Run \`pnpm --filter=get-pnpm run build\`.`)
  }
  const { files } = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'))
  for (const entry of ['package.json', ...files.filter((pattern) => !pattern.startsWith('!'))]) {
    fs.cpSync(path.join(packageDir, entry), path.join(dest, entry), { recursive: true })
  }
}

/** The installed get-pnpm and every package it depends on, by name. */
function downloaderPackages (name = 'get-pnpm', from = WRAPPER_DIR, found = new Map()) {
  if (found.has(name)) {
    return found
  }
  const lookup = createRequire(path.join(from, 'package.json')).resolve.paths(name)
  const packageDir = fs.realpathSync(lookup.map((dir) => path.join(dir, name)).find((dir) => fs.existsSync(dir)))
  found.set(name, packageDir)
  const { dependencies = {} } = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'))
  for (const dependency of Object.keys(dependencies)) {
    downloaderPackages(dependency, packageDir, found)
  }
  return found
}
