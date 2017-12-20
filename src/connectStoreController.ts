import {
  PackageResponse,
  RequestPackageOptions,
  WantedDependency,
} from '@pnpm/package-requester'

import got = require('got')
import pLimit = require('p-limit')
import {StoreController} from 'package-store'

export default function (
  initOpts: {
    remotePrefix: string,
    concurrency?: number,
  },
): Promise<StoreController> {
  const remotePrefix = initOpts.remotePrefix
  const limitedFetch = fetch.bind(null, pLimit(initOpts.concurrency || 100))

  return new Promise((resolve, reject) => {
    resolve({
      close: async () => { return },
      prune: async () => {
        await limitedFetch(`${remotePrefix}/prune`, {})
      },
      requestPackage: requestPackage.bind(null, remotePrefix, limitedFetch),
      saveState: async () => {
        await limitedFetch(`${remotePrefix}/saveState`, {})
      },
      updateConnections: async (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}) => {
        await limitedFetch(`${remotePrefix}/updateConnections`, {
          opts,
          prefix,
        })
      },
    })
  })
}

function fetch(limit: (fn: () => PromiseLike<object>) => Promise<object>, url: string, body: object): Promise<object> { // tslint:disable-line
  return limit(async () => {
    const response = await got(url, {
      body: JSON.stringify(body),
      headers: {'Content-Type': 'application/json'},
      method: 'POST',
      retries: () => {
        return 100
      },
    })
    return JSON.parse(response.body)
  })
}

function requestPackage (
  remotePrefix: string,
  limitedFetch: (url: string, body: object) => any, // tslint:disable-line
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
): Promise<PackageResponse> {
  return limitedFetch(`${remotePrefix}/requestPackage`, {
    options,
    wantedDependency,
  })
  .then((packageResponse: PackageResponse) => {
    const fetchingManifest = limitedFetch(`${remotePrefix}/manifestResponse`, {
      pkgId: packageResponse.id,
    })
    const fetchingFiles = limitedFetch(`${remotePrefix}/packageFilesResponse`, {
      pkgId: packageResponse.id,
    })
    return Object.assign(packageResponse, {
      fetchingFiles,
      fetchingManifest,
      finishing: Promise.all([fetchingManifest, fetchingFiles]).then(() => undefined),
    })
  })
}
