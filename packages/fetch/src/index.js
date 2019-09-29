const createFetchRetry = require('@zeit/fetch-retry')
const nodeFetch = require('node-fetch-unix')

export default createFetchRetry(nodeFetch)

export const FetchError = nodeFetch.FetchError
export const Headers = nodeFetch.Headers
export const Request = nodeFetch.Request
export const Response = nodeFetch.Response
