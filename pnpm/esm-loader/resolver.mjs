import path from 'node:path'
import enhancedResolve from 'enhanced-resolve'

import { loaderError, within } from './store.mjs'

export function createResolver (store) {
  const resolvers = new Map()
  let selected
  const pnpApi = {
    resolveToUnqualified (name, issuer) {
      const owner = store.owner(path.resolve(issuer))
      if (owner?.hasNodeModules && bundles(store, owner, { issuer: path.resolve(issuer), name })) return null
      const dependency = owner?.dependencies.get(name)
      if (dependency === undefined) {
        throw loaderError('ERR_PNPM_LOADER_UNDECLARED_DEPENDENCY', `Package ${owner?.id ?? issuer} does not declare ${name}`)
      }
      selected = store.index(store.packages.get(dependency))
      return selected.root
    },
  }
  return function resolve (specifier, parent, { conditions, filename }) {
    const key = JSON.stringify(conditions)
    let resolver = resolvers.get(key)
    if (!resolver) {
      resolver = enhancedResolve.ResolverFactory.createResolver({
        fileSystem: store.filesystem,
        useSyncFileSystemCalls: true,
        conditionNames: conditions,
        extensions: ['.js', '.json', '.node'],
        mainFields: ['main'],
        mainFiles: ['index'],
        fullySpecified: !conditions.includes('require'),
        symlinks: false,
        pnpApi,
      })
      resolvers.set(key, resolver)
    }
    selected = store.owner(filename ?? parent)
    let resolved
    let failure
    resolver.resolve({}, path.dirname(parent), specifier, {}, (error, result, details) => {
      failure = error
      resolved = details
    })
    if (failure) throw failure
    if (!resolved?.path) throw loaderError('ERR_MODULE_NOT_FOUND', `Cannot resolve ${specifier} from ${parent}`)
    if (selected?.stored && !within(selected.root, resolved.path)) {
      throw loaderError('ERR_PNPM_LOADER_PATH_ESCAPE', `Resolved ${specifier} escapes package ${selected.id}`)
    }
    return resolved
  }
}

/** Whether a `node_modules` directory inside `owner` provides `name` to `issuer`, as Node's lookup would find it before any declared dependency. */
function bundles (store, owner, { issuer, name }) {
  for (let directory = issuer; within(owner.root, directory); directory = path.dirname(directory)) {
    if (store.isDirectory(path.join(directory, 'node_modules', name))) return true
  }
  return false
}
