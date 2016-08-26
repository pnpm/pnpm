'use strict'
const debug = require('debug')('pnpm:http')
const throat = require('throat')
const got = require('got')
const HttpAgent = require('http').Agent
const HttpsAgent = require('https').Agent
const caw = require('caw')
const getAuthToken = require('registry-auth-token')
const getRetrier = require('./get_retrier')

module.exports = opts => {
  opts = opts || {}
  const concurrency = +opts.concurrency || 16
  const forcedRequestOptions = {
    // no worries, the built-in got retries is not used
    // the retry package is used for that purpose
    retries: 0
  }
  const retrier = getRetrier({
    retries: opts.fetchRetries,
    factor: opts.fetchRetryFactor,
    minTimeout: opts.fetchRetryMintimeout,
    maxTimeout: opts.fetchRetryMaxtimeout
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

  const get = retrier((url, options) => {
    const throater = getThroater()
    const key = JSON.stringify([ url, options ])
    if (!cache[key]) {
      cache[key] = new Promise((resolve, reject) => {
        throater(_ => {
          debug(url)
          const promise = got(url, extend(url, options))
          promise.then(resolve).catch(reject)
          return promise
        })
      })
    }
    return cache[key]
  })

  function getJSON (url) {
    return get(url)
      .then(res => {
        const body = JSON.parse(res.body)
        return body
      })
  }

  /*
   * like require('got').stream, but throated
   */

  const getStream = retrier((url, options) => {
    const throater = getThroater()
    return new Promise(resolve => {
      throater(_ => {
        debug(url, '[stream]')
        const stream = got.stream(url, extend(url, options))
        resolve(stream)
        return waiter(stream)
      })
    })
  })

  function waiter (stream) {
    return new Promise((resolve, reject) => {
      stream
        .on('end', resolve)
        .on('error', reject)
    })
  }

  /*
   * Extends `got` options with User Agent headers and stuff
   */

  function extend (url, options) {
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
