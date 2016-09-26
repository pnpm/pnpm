import {IncomingMessage} from 'http'
import pauseStream = require('pause-stream')
import getAuthToken = require('registry-auth-token')

export type RequestParams = {
  headers?: {
    auth: string
  }
}

export type HttpResponse = {
  body: string
}

export type Got = {
  get: (url: string) => Promise<HttpResponse>,
  getStream: (url: string) => Promise<IncomingMessage>,
  getJSON<T>(url: string): Promise<T>
}

export type NpmRegistryClient = {
  get: Function,
  fetch: Function
}

export default (client: NpmRegistryClient): Got => {
  const cache = {}

  function get (url: string, options?: RequestParams) {
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
  function extend (url: string, options?: RequestParams): RequestParams {
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
