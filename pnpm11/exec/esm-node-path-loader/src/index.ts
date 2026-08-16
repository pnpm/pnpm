/**
 * Node.js ignores the `NODE_PATH` environment variable when resolving ESM
 * imports. With a global virtual store, hoisted (phantom) dependencies are
 * reachable only through `NODE_PATH`, because package directories live
 * outside the project, so Node's upward `node_modules` walk from their real
 * paths never reaches the project's hoisted `node_modules`. The resolve hook
 * built here restores `NODE_PATH` lookups for bare specifiers that the
 * default ESM resolution fails to find.
 *
 * The retry goes through the default resolver with a synthetic parent
 * inside each `NODE_PATH` entry, so the caller's export conditions (import
 * vs require) are preserved. Every entry pnpm puts on `NODE_PATH` ends in
 * `node_modules`, which is why the parent walk finds the entry's packages:
 * the resolver checks `<parent dir>/node_modules`, and the entry's parent
 * dir maps straight back to the entry itself.
 *
 * The Rust CLI embeds an identical copy of these sources — the two must
 * stay in sync so both CLIs inject the same `NODE_OPTIONS` value.
 */
const RESOLVE_HELPERS = `\
const nodePaths = (process.env.NODE_PATH ?? '').split(delimiter).filter(Boolean)
const isBareSpecifier = (specifier) => !specifier.startsWith('.') && !specifier.startsWith('/') && !specifier.startsWith('#') && !specifier.includes(':')
`

/*
 * Fallback for Node.js versions that have module.register() but not
 * module.registerHooks() (>=18.19 <22.15): an off-thread hooks module for
 * module.register(). Runtimes with neither API (--import parses from
 * 18.18/19.0) get no hook at all — CJS NODE_PATH resolution still works
 * natively there.
 */
const ASYNC_LOADER_SOURCE = `\
import { delimiter } from 'node:path'
import { pathToFileURL } from 'node:url'
${RESOLVE_HELPERS}
export async function resolve (specifier, context, nextResolve) {
  try {
    return await nextResolve(specifier, context)
  } catch (originalError) {
    if (originalError?.code !== 'ERR_MODULE_NOT_FOUND' || !isBareSpecifier(specifier)) throw originalError
    for (const nodePath of nodePaths) {
      try {
        return await nextResolve(specifier, { ...context, parentURL: pathToFileURL(nodePath + '/x').href })
      } catch (fallbackError) {
        if (fallbackError?.code !== 'ERR_MODULE_NOT_FOUND') throw fallbackError
      }
    }
    throw originalError
  }
}
`

const REGISTRATION_SOURCE = `\
if (process.env.NODE_PATH) {
  const { register, registerHooks } = await import('node:module')
  const { delimiter } = await import('node:path')
  const { pathToFileURL } = await import('node:url')
  if (registerHooks) {
    ${RESOLVE_HELPERS.replaceAll('\n', '\n    ').trimEnd()}
    registerHooks({
      resolve (specifier, context, nextResolve) {
        try {
          return nextResolve(specifier, context)
        } catch (originalError) {
          if (originalError?.code !== 'ERR_MODULE_NOT_FOUND' || !isBareSpecifier(specifier)) throw originalError
          for (const nodePath of nodePaths) {
            try {
              return nextResolve(specifier, { ...context, parentURL: pathToFileURL(nodePath + '/x').href })
            } catch (fallbackError) {
              if (fallbackError?.code !== 'ERR_MODULE_NOT_FOUND') throw fallbackError
            }
          }
          throw originalError
        }
      },
    })
  } else if (register) {
    register(${JSON.stringify(`data:text/javascript,${strictUriEncode(ASYNC_LOADER_SOURCE)}`)})
  }
}
`

/*
 * Encodes everything outside the RFC 3986 unreserved set. Unlike
 * encodeURIComponent, this also encodes ! ' ( ) * — the single quote in
 * particular, which Node's NODE_OPTIONS tokenizer treats as a quote
 * delimiter and would strip from the flag.
 */
function strictUriEncode (text: string): string {
  return encodeURIComponent(text).replace(/[!'()*]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)
}

/*
 * The hooks are inlined into the flag as data: URLs, so the flag is a
 * self-contained constant: no file has to exist on disk for the child
 * Node.js process to load it, and it stays valid no matter which project or
 * pnpm version spawned the child. When NODE_PATH is empty, the registration
 * module exits without installing any hook.
 */
export const esmNodePathLoaderImportFlag = `--import=data:text/javascript,${strictUriEncode(REGISTRATION_SOURCE)}`

/**
 * Reapplies the loader flag after `nodeOptions` from config replaces a
 * previously built `NODE_OPTIONS` value that carried it.
 */
export function keepEsmNodePathLoaderOption (nodeOptions: string, previousNodeOptions: string | undefined): string {
  if (previousNodeOptions?.includes(esmNodePathLoaderImportFlag)) {
    return addEsmNodePathLoaderOption(nodeOptions)
  }
  return nodeOptions
}

export function addEsmNodePathLoaderOption (nodeOptions: string | undefined): string {
  if (!nodeOptions) return esmNodePathLoaderImportFlag
  if (nodeOptions.includes(esmNodePathLoaderImportFlag)) return nodeOptions
  return `${nodeOptions} ${esmNodePathLoaderImportFlag}`
}
