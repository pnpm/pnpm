import {
  FetchPackageToStoreOptions,
  PackageFilesResponse,
  PackageResponse,
  PackageUsagesBySearchQueries,
  RequestPackageOptions,
  StoreController,
  WantedDependency,
} from '@pnpm/store-controller-types'
import { PackageManifest } from '@pnpm/types'

import got = require('got')
import pLimit = require('p-limit')
import uuid = require('uuid')

export type StoreServerController = StoreController & {
  stop (): Promise<void>,
}

export default function (
  initOpts: {
    remotePrefix: string,
    concurrency?: number,
  },
): Promise<StoreServerController> {
  const remotePrefix = initOpts.remotePrefix
  const limitedFetch = fetch.bind(null, pLimit(initOpts.concurrency || 100))

  return new Promise((resolve, reject) => {
    resolve({
      close: async () => { return },
      fetchPackage: fetchPackage.bind(null, remotePrefix, limitedFetch),
      findPackageUsages: async (searchQueries: string[]): Promise<PackageUsagesBySearchQueries> => {
        return limitedFetch(`${remotePrefix}/findPackageUsages`, { searchQueries })
      },
      getPackageLocation: async (
        packageId: string,
        packageName: string,
        opts: {
          importerPrefix: string,
          targetEngine?: string,
        },
      ): Promise<{ directory: string, isBuilt: boolean }> => {
        return limitedFetch(`${remotePrefix}/getPackageLocation`, {
          opts,
          packageId,
          packageName,
        })
      },
      importPackage: async (from: string, to: string, opts: {
        filesResponse: PackageFilesResponse,
        force: boolean,
      }) => {
        await limitedFetch(`${remotePrefix}/importPackage`, {
          from,
          opts,
          to,
        })
      },
      prune: async () => {
        await limitedFetch(`${remotePrefix}/prune`, {})
      },
      requestPackage: requestPackage.bind(null, remotePrefix, limitedFetch),
      saveState: async () => {
        await limitedFetch(`${remotePrefix}/saveState`, {})
      },
      stop: () => limitedFetch(`${remotePrefix}/stop`, {}),
      updateConnections: async (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}) => {
        await limitedFetch(`${remotePrefix}/updateConnections`, {
          opts,
          prefix,
        })
      },
      upload: async (builtPkgLocation: string, opts: {pkgId: string, engine: string}) => {
        await limitedFetch(`${remotePrefix}/upload`, {
          builtPkgLocation,
          opts,
        })
      },
    })
  })
}

function fetch(limit: (fn: () => PromiseLike<object>) => Promise<object>, url: string, body: object): Promise<object | undefined> { // tslint:disable-line
  return limit(async () => {
    try {
      const response = await got(url, {
        body: JSON.stringify(body),
        headers: { 'Content-Type': 'application/json' },
        method: 'POST',
        retries: () => {
          return 100
        },
      })
      if (!response.body) {
        return undefined
      }
      return JSON.parse(response.body)
    } catch (e) {
      throw JSON.parse(e.response.body)
    }
  })
}

function requestPackage (
  remotePrefix: string,
  limitedFetch: (url: string, body: object) => any, // tslint:disable-line
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
): Promise<PackageResponse> {
  const msgId = uuid.v4()

  return limitedFetch(`${remotePrefix}/requestPackage`, {
    msgId,
    options,
    wantedDependency,
  })
  .then((packageResponseBody: object) => {
    const fetchingRawManifest = !packageResponseBody['fetchingRawManifestInProgress'] // tslint:disable-line
      ? undefined
      : limitedFetch(`${remotePrefix}/rawManifestResponse`, {
        msgId,
      })
    delete packageResponseBody['fetchingRawManifestInProgress'] // tslint:disable-line

    if (options.skipFetch) {
      return {
        body: packageResponseBody,
        fetchingRawManifest,
      }
    }

    const fetchingFiles = limitedFetch(`${remotePrefix}/packageFilesResponse`, {
      msgId,
    })
    return {
      body: packageResponseBody,
      fetchingFiles,
      fetchingRawManifest,
      finishing: Promise.all([fetchingRawManifest, fetchingFiles]).then(() => undefined),
    }
  })
}

function fetchPackage (
  remotePrefix: string,
  limitedFetch: (url: string, body: object) => any, // tslint:disable-line
  options: FetchPackageToStoreOptions,
): Promise<{
  fetchingFiles: Promise<PackageFilesResponse>,
  fetchingFullManifest?: Promise<PackageManifest>,
  finishing: Promise<void>,
  inStoreLocation: string,
}> {
  const msgId = uuid.v4()

  return limitedFetch(`${remotePrefix}/fetchPackage`, {
    msgId,
    options,
  })
  .then((fetchResponseBody: object & {inStoreLocation: string}) => {
    const fetchingRawManifest = options.fetchRawManifest
      ? limitedFetch(`${remotePrefix}/rawManifestResponse`, { msgId })
      : undefined

    const fetchingFiles = limitedFetch(`${remotePrefix}/packageFilesResponse`, {
      msgId,
    })
    return {
      fetchingFiles,
      fetchingRawManifest,
      finishing: Promise.all([fetchingRawManifest, fetchingFiles]).then(() => undefined),
      inStoreLocation: fetchResponseBody.inStoreLocation,
    }
  })
}
