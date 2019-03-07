import createFetchRetry = require('@zeit/fetch-retry')
import nodeFetch = require('node-fetch')

export default createFetchRetry(nodeFetch)
