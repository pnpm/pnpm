import RegClient = require('npm-registry-client')
import {IncomingMessage} from 'http'
import pauseStream = require('pause-stream')
import getAuthToken = require('registry-auth-token')
import logger = require('@zkochan/logger')
import defaults from '../defaults'

export type GotOptions = {
  fetchRetries?: number,
  fetchRetryFactor?: number,
  fetchRetryMintimeout?: number,
  fetchRetryMaxtimeout?: number
}

export type RequestParams = {
  headers?: {
    auth: string
  }
}

export type HttpResponse = {
  body: string
}

export type GetFunc = (url: string, options?: GotOptions) => Promise<HttpResponse>

export type Got = {
  get: GetFunc,
  getStream: (url: string, options?: GotOptions) => Promise<IncomingMessage>,
  getJSON<T>(url: string): Promise<T>
}

export default (opts: GotOptions): Got => {
  opts = opts || {}

  const client = new RegClient({
    retry: {
      count: opts.fetchRetries || defaults.fetchRetries,
      factor: opts.fetchRetryFactor || defaults.fetchRetryFactor,
      minTimeout: opts.fetchRetryMintimeout || defaults.fetchRetryMintimeout,
      maxTimeout: opts.fetchRetryMaxtimeout || defaults.fetchRetryMaxtimeout
    },
    log: Object.assign({}, logger, {
      verbose: logger.log.bind(null, 'verbose'),
      http: logger.log.bind(null, 'http')
    })
  })

  const cache = {}

  const get: GetFunc = (url: string, options?: RequestParams) => {
    const key = JSON.stringify([ url, options ])
    if (!cache[key]) {
      cache[key] = new Promise((resolve, reject) => {
        client.get(url, extend(url, options), (err: Error, data: Object, raw: Object, res: HttpResponse) => {
          if (err) return reject(err)
          resolve(res)
        })
      })
    }
    return cache[key]
  }

  function getJSON (url: string, options?: RequestParams) {
    const key = JSON.stringify([ url, options ])
    if (!cache[key]) {
      cache[key] = new Promise((resolve, reject) => {
        client.get(url, extend(url, options), (err: Error, data: Object, raw: Object, res: HttpResponse) => {
          if (err) return reject(err)
          resolve(data)
        })
      })
    }
    return cache[key]
  }

  const getStream = function (url: string, options?: RequestParams): Promise<IncomingMessage> {
    return new Promise((resolve, reject) => {
      client.fetch(url, extend(url, options), (err: Error, res: IncomingMessage) => {
        if (err) return reject(err)
        const ps = pauseStream()
        res.pipe(ps.pause())
        resolve(ps)
      })
    })
  }

  /**
   * Extends request options with authorization headers
   */
  function extend (url: string, options?: RequestParams): GotOptions {
    options = options || {}
    const authToken = getAuthToken(url, {recursive: true})
    if (authToken) {
      options.headers = Object.assign({}, options.headers, {
        authorization: `${authToken.type} ${authToken.token}`
      })
    }
    return options
  }

  return {
    get: get,
    getJSON: getJSON,
    getStream: getStream
  }
}
