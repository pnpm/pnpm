// The entry point loads get-pnpm from the wrapper's `dist/node_modules`, where
// scripts/bundle-node-gyp.mjs puts it and its dependencies for the release. A
// fixture that runs the entry point stands in for that.
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** Copies the installed get-pnpm and its dependencies into `wrapperDir`'s `dist/node_modules`. */
export function bundleDownloader (wrapperDir) {
  for (const [name, packageDir] of downloaderPackages()) {
    fs.cpSync(packageDir, path.join(wrapperDir, 'dist/node_modules', name), { dereference: true, recursive: true })
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
