import createDebug from '../debug'
const debug = createDebug('pnpm:http')
import throat = require('throat')
import got = require('got')
import { Agent as HttpAgent } from 'http'
import { Agent as HttpsAgent } from 'https'
import caw = require('caw')
import getAuthToken = require('registry-auth-token')
import getRetrier from './get_retrier'
import defaults from '../defaults'

export type GotOptions = {
  fetchRetries?: number,
  fetchRetryFactor?: number,
  fetchRetryMintimeout?: number,
  fetchRetryMaxtimeout?: number,
  concurrency?: number,
  headers?: {
    authorization: string
  },
  agent?: HttpAgent
}

export type HttpResponse = {
  body: string
}

export type GetFunc = (url: string, options?: GotOptions) => Promise<HttpResponse>

export type Got = {
  get: GetFunc,
  getStream: (url: string, options?: GotOptions) => Promise<NodeJS.ReadableStream>,
  getJSON<T>(url: string): Promise<T>
}

export default (opts: GotOptions): Got => {
  opts = opts || {}
  const concurrency = +opts.concurrency || 16
  const forcedRequestOptions = {
    // no worries, the built-in got retries is not used
    // the retry package is used for that purpose
    retries: 0
  }
  const sharedOpts = opts
  const retrier = getRetrier({
    retries: opts.fetchRetries || defaults.fetchRetries,
    factor: opts.fetchRetryFactor || defaults.fetchRetryFactor,
    minTimeout: opts.fetchRetryMintimeout || defaults.fetchRetryMintimeout,
    maxTimeout: opts.fetchRetryMaxtimeout || defaults.fetchRetryMaxtimeout
  })

  const cache = {}

  function getThroater () {
    return throat(concurrency)
  }

  const httpKeepaliveAgent = new HttpAgent({
    keepAlive: true,
    keepAliveMsecs: 30000
  })
  const httpsKeepaliveAgent = new HttpsAgent({
    keepAlive: true,
    keepAliveMsecs: 30000
  })

  /*
   * waits in line
   */

  const get: GetFunc = retrier((url: string, options?: GotOptions) => {
    const throater = getThroater()
    const key = JSON.stringify([ url, options ])
    if (!cache[key]) {
      cache[key] = throater(() => {
        debug(url)
        return got(url, extend(url, options || sharedOpts))
      })
    }
    return cache[key]
  })

  function getJSON (url: string) {
    return get(url)
      .then((res: HttpResponse) => {
        const body = JSON.parse(res.body)
        return body
      })
  }

  /*
   * like require('got').stream, but throated
   */

  const getStream = retrier((url: string, options?: GotOptions) => {
    const throater = getThroater()
    return new Promise((resolve, reject) => {
      throater(() => {
        debug(url, '[stream]')
        const stream = got.stream(url, extend(url, options || sharedOpts))
        resolve(stream)
        return waiter(stream)
      })
      .catch(reject)
    })
  })

  function waiter (stream: NodeJS.ReadableStream) {
    return new Promise((resolve, reject) => {
      stream
        .on('end', resolve)
        .on('error', reject)
    })
  }

  /*
   * Extends `got` options with User Agent headers and stuff
   */

  function extend (url: string, options: GotOptions): GotOptions {
    if (!options) options = Object.assign({}, forcedRequestOptions)
    if (url.indexOf('https://') === 0) {
      options.agent = caw({ protocol: 'https' }) || httpsKeepaliveAgent

      const authToken = getAuthToken(url, {recursive: true})
      if (authToken) {
        options.headers = Object.assign({}, options.headers, {
          authorization: authToken.type + ' ' + authToken.token
        })
      }
    } else {
      options.agent = caw({ protocol: 'http' }) || httpKeepaliveAgent
    }
    return Object.assign({}, options, forcedRequestOptions, {
      headers: Object.assign({}, options.headers, {
        'user-agent': 'https://github.com/rstacruz/pnpm'
      })
    })
  }

  return {
    get: get,
    getJSON: getJSON,
    getStream: getStream
  }
}
