import module, { isBuiltin } from 'node:module'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL, URL } from 'node:url'

import { createResolver } from './resolver.mjs'
import { loaderError, openStore, within } from './store.mjs'

/** Register a version 1 store manifest. Returns Node's deregisterable hook handle. */
export function registerStoreLoader (manifestURL) {
  assertSupportedNode(process.versions.node)
  // Read from the namespace after the version check: Node.js releases without
  // `registerHooks` would fail a named import while the module links.
  return module.registerHooks(createStoreHooks(manifestURL))
}

/**
 * Check that a Node.js version can run the store loader.
 *
 * @param {string} version A version as `process.versions.node` reports it, such as `24.18.0` or `27.0.0-nightly20261001abcdef`.
 * @throws {Error} `ERR_PNPM_LOADER_UNSUPPORTED_NODE` unless the version satisfies `^24.18.0 || >=26.2.0`.
 * A prerelease of 24.18.0 or 26.2.0 is below that range and is rejected. On the rejected versions,
 * CommonJS that ESM imports resolves its `require()` calls without the loader's resolve hooks.
 */
export function assertSupportedNode (version) {
  const [release, prerelease] = version.split('-', 2)
  const [major, minor, patch] = release.split('.').map(Number)
  const atLeast = (minimumMinor) => minor > minimumMinor || (minor === minimumMinor && (patch > 0 || prerelease === undefined))
  if ((major === 24 && atLeast(18)) || (major === 26 && atLeast(2)) || major > 26) return
  throw loaderError(
    'ERR_PNPM_LOADER_UNSUPPORTED_NODE',
    `The pnpm store loader requires Node.js ^24.18.0 or >=26.2.0, but this is Node.js ${version}. Use a supported Node.js version, or install with a different nodeLinker.`
  )
}

export function createStoreHooks (manifestURL) {
  const store = openStore(manifestURL)
  const resolvePackage = createResolver(store)
  const manifests = new Map()
  return {
    resolve (specifier, context, nextResolve) {
      if (!context.parentURL && !isBuiltin(specifier)) {
        const entry = storedEntry(specifier, store, manifests)
        if (entry) return entry
      }
      if (isBuiltin(specifier) || !context.parentURL?.startsWith('file:')) {
        return nextResolve(specifier, context)
      }
      const parent = fileURLToPath(context.parentURL)
      const owner = store.owner(parent)
      if (!owner) return nextResolve(specifier, context)
      const request = fileRequest(specifier, context)
      if (request === null) return nextResolve(specifier, context)
      if (owner.resolution === 'node' && (!request.filename || !within(store.virtualRoot, request.filename))) {
        return nextResolve(specifier, context)
      }
      if (request.filename && (!owner.stored || !request.relative) && !within(store.virtualRoot, request.filename)) {
        return nextResolve(specifier, context)
      }
      if (owner.stored && request.relative && !within(owner.root, request.filename)) {
        throw loaderError('ERR_PNPM_LOADER_PATH_ESCAPE', `Import ${specifier} escapes package ${owner.id}`)
      }
      const resolved = resolvePackage(request.specifier, parent, { conditions: context.conditions, filename: request.filename })
      const url = pathToFileURL(resolved.path)
      url.search = request.search || resolved.query || ''
      url.hash = request.hash || resolved.fragment || ''
      if (within(store.virtualRoot, resolved.path)) {
        moduleFormat(resolved.path, store, manifests)
      }
      return { url: url.href, shortCircuit: true }
    },
    load (url, context, nextLoad) {
      if (!url.startsWith('file:')) return nextLoad(url, context)
      const filename = fileURLToPath(url)
      if (!within(store.virtualRoot, filename)) return nextLoad(url, context)
      const format = moduleFormat(filename, store, manifests)
      if (format === 'json' && !context.conditions.includes('require') && context.importAttributes?.type !== 'json') {
        throw loaderError('ERR_IMPORT_ATTRIBUTE_MISSING', `Module ${url} needs an import attribute of type json`)
      }
      validateAttributes(format, context.importAttributes, url)
      return { format, source: store.filesystem.readFileSync(filename), shortCircuit: true }
    },
  }
}

function fileRequest (specifier, context) {
  const dotDirectory = context.conditions.includes('require') && (specifier === '.' || specifier === '..')
  const relative = dotDirectory || specifier.startsWith('./') || specifier.startsWith('../')
  const nativeAbsolute = path.isAbsolute(specifier) && !specifier.startsWith('/')
  if (nativeAbsolute || (context.conditions.includes('require') && (relative || path.isAbsolute(specifier)))) {
    const filename = path.resolve(path.dirname(fileURLToPath(context.parentURL)), specifier)
    return { filename, relative, specifier: escapeFilename(filename) }
  }
  if (specifier.startsWith('file:') || specifier.startsWith('./') || specifier.startsWith('../') || specifier.startsWith('/')) {
    const url = new URL(specifier, context.parentURL)
    const filename = fileURLToPath(url)
    return { filename, relative, specifier: escapeFilename(filename), search: url.search, hash: url.hash }
  }
  if (specifier.includes(':')) return null
  if (!specifier.startsWith('#') && (specifier.includes('\\') || specifier.split('/').includes('..'))) {
    throw loaderError('ERR_PNPM_LOADER_PATH_ESCAPE', `Invalid package subpath: ${specifier}`)
  }
  return { specifier }
}

function escapeFilename (filename) {
  return filename.replaceAll('#', '\0#').replaceAll('?', '\0?')
}

function moduleFormat (filename, store, manifests) {
  const extension = path.extname(filename)
  if (extension === '.mjs') return 'module'
  if (extension === '.cjs') return 'commonjs'
  if (extension === '.json') return 'json'
  if (extension !== '.js' && extension !== '') {
    throw loaderError('ERR_PNPM_LOADER_UNSUPPORTED_FORMAT', `Cannot load ${extension || 'extensionless'} store file: ${filename}`)
  }
  const owner = store.owner(filename)
  if (!owner) throw loaderError('ENOENT', `No package in store manifest: ${filename}`)
  for (let directory = path.dirname(filename); within(owner.root, directory); directory = path.dirname(directory)) {
    const manifestPath = path.join(directory, 'package.json')
    if (!store.file(manifestPath)) continue
    if (!manifests.has(manifestPath)) {
      manifests.set(manifestPath, store.filesystem.readJsonSync(manifestPath))
    }
    return manifests.get(manifestPath).type === 'module' ? 'module' : 'commonjs'
  }
  return 'commonjs'
}

function validateAttributes (format, attributes = {}, url) {
  for (const [name, value] of Object.entries(attributes)) {
    if (name !== 'type' || value !== 'json') {
      throw loaderError('ERR_IMPORT_ATTRIBUTE_UNSUPPORTED', `Unsupported import attribute ${name}=${value} for ${url}`)
    }
  }
  if (format !== 'json' && attributes.type === 'json') {
    throw loaderError('ERR_IMPORT_ATTRIBUTE_TYPE_INCOMPATIBLE', `Module ${url} is not JSON`)
  }
}

function storedEntry (specifier, store, manifests) {
  const url = specifier.startsWith('file:') ? new URL(specifier) : path.isAbsolute(specifier) ? pathToFileURL(specifier) : null
  if (!url || !store.file(fileURLToPath(url))) return null
  return { url: url.href, format: moduleFormat(fileURLToPath(url), store, manifests), shortCircuit: true }
}
