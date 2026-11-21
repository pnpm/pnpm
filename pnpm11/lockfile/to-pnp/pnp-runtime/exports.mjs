import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

import { packageExportsResolve } from 'yarn-pnp-exports'
import { getOptionValue } from 'yarn-pnp-node-options'

const defaultConditions = new Set(['node', 'require', ...getOptionValue('--conditions')])
const exportsByManifest = new Map()

export function resolvePackageExports ({ packageLocation, packageSubpath, issuer, conditions = defaultConditions }) {
  const manifestPath = path.join(packageLocation, 'package.json')
  const exports = readExports(manifestPath)
  if (exports == null) return null
  return fileURLToPath(packageExportsResolve({
    packageJSONUrl: pathToFileURL(manifestPath),
    packageSubpath,
    exports,
    base: issuer ? pathToFileURL(path.resolve(issuer)) : null,
    conditions,
  }))
}

function readExports (manifestPath) {
  if (exportsByManifest.has(manifestPath)) return exportsByManifest.get(manifestPath)
  let exports
  try {
    exports = JSON.parse(fs.readFileSync(manifestPath, 'utf8')).exports
  } catch (error) {
    if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error
  }
  exportsByManifest.set(manifestPath, exports)
  return exports
}
