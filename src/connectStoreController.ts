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
  .then((packageResponseBody: object) => {
    const fetchingManifest = packageResponseBody['manifest'] // tslint:disable-line
      ? undefined
      : limitedFetch(`${remotePrefix}/manifestResponse`, {
          pkgId: packageResponseBody['id'], // tslint:disable-line
        })

    if (options.skipFetch) {
      return {
        body: packageResponseBody,
        fetchingManifest,
      }
    }

    const fetchingFiles = limitedFetch(`${remotePrefix}/packageFilesResponse`, {
      pkgId: packageResponseBody['id'], // tslint:disable-line
    })
    return {
      body: packageResponseBody,
      fetchingFiles,
      fetchingManifest,
      finishing: Promise.all([fetchingManifest, fetchingFiles]).then(() => undefined),
    }
  })
}
