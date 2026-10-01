import { isBuiltin, registerHooks } from 'node:module'
import path from 'node:path'
import { fileURLToPath, pathToFileURL, URL } from 'node:url'

import { createResolver } from './resolver.mjs'
import { loaderError, openStore, within } from './store.mjs'

/** Register a version 1 store manifest. Returns Node's deregisterable hook handle. */
export function registerStoreLoader (manifestURL) {
  return registerHooks(createStoreHooks(manifestURL))
}

export function createStoreHooks (manifestURL) {
  const store = openStore(manifestURL)
  const resolvePackage = createResolver(store)
  const manifests = new Map()
  return {
    resolve (specifier, context, nextResolve) {
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
      const resolved = resolvePackage(request.specifier, parent, context.conditions)
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
  if (extension !== '.js') {
    throw loaderError('ERR_PNPM_LOADER_UNSUPPORTED_FORMAT', `Cannot load ${extension || 'extensionless'} store file: ${filename}`)
  }
  const owner = store.owner(filename)
  for (let directory = path.dirname(filename); within(owner.root, directory); directory = path.dirname(directory)) {
    const manifestPath = path.join(directory, 'package.json')
    if (!store.files.has(manifestPath)) continue
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
