import { fetch } from '@pnpm/fetch'
import {
  type PkgRequestFetchResult,
  type FetchPackageToStoreOptions,
  type PackageFilesResponse,
  type PackageResponse,
  type RequestPackageOptions,
  type StoreController,
  type WantedDependency,
} from '@pnpm/store-controller-types'

import pLimit from 'p-limit'
import pShare from 'promise-share'
import { omit } from 'ramda'
import { v4 as uuidv4 } from 'uuid'

function replacer (key: unknown, value: unknown) {
  if (value instanceof Map || Object.prototype.toString.call(value) === '[object Map]') {
    return {
      dataType: 'Map',
      // @ts-expect-error
      value: Array.from(value.entries()),
    }
  }
  return value
}

function reviver (key: unknown, value: unknown) {
  if (typeof value === 'object' && value !== null) {
    // @ts-expect-error
    if (value.dataType === 'Map') {
      // @ts-expect-error
      return new Map(value.value)
    }
  }
  return value
}

export interface StoreServerController extends StoreController {
  stop: () => Promise<void>
}

export async function connectStoreController (
  initOpts: {
    remotePrefix: string
    concurrency?: number
  }
): Promise<StoreServerController> {
  const remotePrefix = initOpts.remotePrefix
  const limitedFetch = limitFetch.bind(null, pLimit(initOpts.concurrency ?? 100))

  return new Promise((resolve, reject) => {
    resolve({
      close: async () => { },
      fetchPackage: fetchPackage.bind(null, remotePrefix, limitedFetch),
      getFilesIndexFilePath: () => ({ filesIndexFile: '', target: '' }), // NOT IMPLEMENTED
      importPackage: async (to: string, opts: {
        filesResponse: PackageFilesResponse
        force: boolean
      }) => {
        return limitedFetch(`${remotePrefix}/importPackage`, {
          opts,
          to,
        }) as Promise<{ importMethod: string | undefined, isBuilt: boolean }>
      },
      prune: async () => {
        await limitedFetch(`${remotePrefix}/prune`, {})
      },
      requestPackage: requestPackage.bind(null, remotePrefix, limitedFetch),
      stop: async () => {
        await limitedFetch(`${remotePrefix}/stop`, {})
      },
      upload: async (builtPkgLocation: string, opts: { filesIndexFile: string, sideEffectsCacheKey: string }) => {
        await limitedFetch(`${remotePrefix}/upload`, {
          builtPkgLocation,
          opts,
        })
      },
      clearResolutionCache: () => {},
    })
  })
}

function limitFetch<T>(limit: (fn: () => PromiseLike<T>) => Promise<T>, url: string, body: object): Promise<T> { // eslint-disable-line
  return limit(async () => {
    // TODO: the http://unix: should be also supported by the fetcher
    // but it fails with node-fetch-unix as of v2.3.0
    if (url.startsWith('http://unix:')) {
      url = url.replace('http://unix:', 'unix:')
    }
    const response = await fetch(url, {
      body: JSON.stringify(body, replacer),
      headers: { 'Content-Type': 'application/json' },
      method: 'POST',
      retry: {
        retries: 100,
      },
    })
    if (!response.ok) {
      throw await response.json()
    }
    const json = JSON.parse(await response.text(), reviver) as any // eslint-disable-line
    if (json.error) {
      throw json.error
    }
    return json as T
  })
}

async function requestPackage (
  remotePrefix: string,
  limitedFetch: (url: string, body: object) => any, // eslint-disable-line
  wantedDependency: WantedDependency,
  options: RequestPackageOptions
): Promise<PackageResponse> {
  const msgId = uuidv4()
  const packageResponseBody = await limitedFetch(`${remotePrefix}/requestPackage`, {
    msgId,
    options: omit(['allowBuild', 'onFetchError'], options),
    wantedDependency,
  })
  if (options.skipFetch === true) {
    return { body: packageResponseBody }
  }
  const fetchingFiles = limitedFetch(`${remotePrefix}/packageFilesResponse`, {
    msgId,
  })
  return {
    body: packageResponseBody,
    fetching: pShare(fetchingFiles),
  }
}

async function fetchPackage (
  remotePrefix: string,
  limitedFetch: (url: string, body: object) => any, // eslint-disable-line
  options: FetchPackageToStoreOptions
): Promise<{
    fetching: () => Promise<PkgRequestFetchResult>
    filesIndexFile: string
    inStoreLocation: string
  }> {
  const msgId = uuidv4()

  const fetchResponseBody = await limitedFetch(`${remotePrefix}/fetchPackage`, {
    msgId,
    options: omit(['allowBuild', 'onFetchError'], options),
  }) as object & { filesIndexFile: string, inStoreLocation: string }
  const fetching = limitedFetch(`${remotePrefix}/packageFilesResponse`, {
    msgId,
  })
  return {
    fetching: pShare(fetching),
    filesIndexFile: fetchResponseBody.filesIndexFile,
    inStoreLocation: fetchResponseBody.inStoreLocation,
  }
}
