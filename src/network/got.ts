import {IncomingMessage} from 'http'
import pauseStream = require('pause-stream')
import getRegistryAuthInfo = require('registry-auth-token')
import memoize = require('lodash.memoize')
import pLimit = require('p-limit')

export type RequestParams = {
  auth?: {
    token: string
  } | {
    username: string,
    password: string
  }
}

export type HttpResponse = {
  body: string
}

export type Got = {
  getStream: (url: string) => Promise<IncomingMessage>,
  getJSON<T>(url: string): Promise<T>,
}

export type NpmRegistryClient = {
  get: Function,
  fetch: Function
}

export default (client: NpmRegistryClient, opts: {networkConcurrency: number}): Got => {
  const limit = pLimit(opts.networkConcurrency)

  async function getJSON (url: string) {
    return limit(() => new Promise((resolve, reject) => {
      client.get(url, createOptions(url), (err: Error, data: Object, raw: Object, res: HttpResponse) => {
        if (err) return reject(err)
        resolve(data)
      })
    }))
  }

  const getStream = function (url: string): Promise<IncomingMessage> {
    return limit(() => new Promise((resolve, reject) => {
      client.fetch(url, createOptions(url), (err: Error, res: IncomingMessage) => {
        if (err) return reject(err)
        const ps = pauseStream()
        // without pausing, gunzip/tar-fs would miss the beginning of the stream
        res.pipe(ps.pause())
        resolve(ps)
      })
    }))
  }

  function createOptions (url: string): RequestParams {
    const authInfo = getRegistryAuthInfo(url, {recursive: true})
    if (!authInfo) return {}
    switch (authInfo.type) {
      case 'Bearer':
        return {
          auth: {
            token: authInfo.token
          }
        }
      case 'Basic':
        return {
          auth: {
            username: authInfo.username,
            password: authInfo.password
          }
        }
      default:
        throw new Error(`Unsupported authorization type '${authInfo.type}'`)
    }
  }

  return {
    getJSON: memoize(getJSON),
    getStream: getStream,
  }
}
