import createFetchRetry = require('@zeit/fetch-retry')
import nodeFetch = require('node-fetch-unix')

export default createFetchRetry(nodeFetch)
