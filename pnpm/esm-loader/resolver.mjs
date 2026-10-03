import path from 'node:path'
import enhancedResolve from 'enhanced-resolve'

import { loaderError } from './store.mjs'

export function createResolver (store) {
  const resolvers = new Map()
  const pnpApi = {
    resolveToUnqualified (name, issuer) {
      const owner = store.owner(path.resolve(issuer))
      const dependency = owner?.dependencies.get(name)
      if (dependency === undefined) {
        throw loaderError('ERR_PNPM_LOADER_UNDECLARED_DEPENDENCY', `Package ${owner?.id ?? issuer} does not declare ${name}`)
      }
      return store.packages.get(dependency).root
    },
  }
  return function resolve (specifier, parent, conditions) {
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
    let resolved
    let failure
    resolver.resolve({}, path.dirname(parent), specifier, {}, (error, result, details) => {
      failure = error
      resolved = details
    })
    if (failure) throw failure
    if (!resolved?.path) throw loaderError('ERR_MODULE_NOT_FOUND', `Cannot resolve ${specifier} from ${parent}`)
    return resolved
  }
}
