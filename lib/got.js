'use strict'
const debug = require('debug')('pnpm:http')
const throat = require('throat')
const got = require('got')
const HttpAgent = require('http').Agent
const HttpsAgent = require('https').Agent
const caw = require('caw')
const getAuthToken = require('registry-auth-token')

const cache = {}

function getThroater () {
  return throat(+process.env.pnpm_config_concurrency)
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

exports.get = (url, options) => {
  const throater = getThroater()
  const key = JSON.stringify([ url, options ])
  if (!cache[key]) {
    cache[key] = new Promise(resolve => {
      throater(_ => {
        debug(url)
        const promise = got(url, extend(url, options))
        resolve({ promise })
        return promise
      })
    })
  }
  return cache[key]
}

/*
 * like require('got').stream, but throated
 */

exports.getStream = (url, options) => {
  const throater = getThroater()
  return new Promise(resolve => {
    throater(_ => {
      debug(url, '[stream]')
      const stream = got.stream(url, extend(url, options))
      resolve(stream)
      return waiter(stream)
    })
  })
}

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
  if (!options) options = {}
  if (url.indexOf('https://') === 0) {
    options.agent = caw() || httpsKeepaliveAgent

    const authToken = getAuthToken(url, {recursive: true})
    if (authToken) {
      options.headers = Object.assign({}, options.headers || {}, {
        authorization: authToken.type + ' ' + authToken.token
      })
    }
  } else {
    options.agent = caw() || httpKeepaliveAgent
  }
  return Object.assign({}, options, {
    headers: Object.assign({}, options.headers || {}, {
      'user-agent': 'https://github.com/rstacruz/pnpm'
    })
  })
}
