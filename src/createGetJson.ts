import logger from '@pnpm/logger'
import {IncomingMessage} from 'http'
import RegClient = require('npm-registry-client')

export interface HttpResponse {
  body: string
}

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void,
  fetch: (url: string, opts: {auth?: object}, cb: (err: Error, res: IncomingMessage) => void) => void,
}

export default (
  gotOpts: {
    proxy?: {
      http?: string,
      https?: string,
      localAddress?: string,
    },
    ssl?: {
      certificate?: string,
      key?: string,
      ca?: string,
      strict?: boolean,
    },
    retry?: {
      count?: number,
      factor?: number,
      minTimeout?: number,
      maxTimeout?: number,
      randomize?: boolean,
    },
    userAgent?: string,
  },
): <T>(url: string, registry: string) => Promise<T> => {
  const registryLog = logger('registry')
  const client = new RegClient({
    ...gotOpts,
    log: {
      ...registryLog,
      http: registryLog.debug.bind(null, 'http'),
      verbose: registryLog.debug.bind(null, 'http'),
    },
  })

  return function getJSON<T> (url: string, registry: string, auth?: object): Promise<T> {
    return new Promise((resolve, reject) => {
      const getOpts = {
        auth,
        fullMetadata: false,
      }
      client.get(url, getOpts, (err: Error, data: object, raw: object, res: HttpResponse) => {
        if (err) {
          reject(err)
          return
        }
        resolve(data as any) // tslint:disable-line
      })
    })
  }
}
