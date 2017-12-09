'use strict'
const connectPackageRequester = require('@pnpm/server').connectPackageRequester

main()
  .then(() => console.log('Done'))
  .catch(err => console.error(err))

 async function main () {
  const port = 5813
  const hostname = '127.0.0.1'
  const registry = 'https://registry.npmjs.org/'
  const requestPackage = await connectPackageRequester({port, hostname})
  const response = await requestPackage(
    {alias: 'is-positive', pref: '1.0.0'},
    {
      downloadPriority: 0,
      loggedPkg: {rawSpec: 'sfdf'},
      offline: false,
      prefix: process.cwd(),
      registry,
      verifyStoreIntegrity: false,
    }
  )

  console.log(response)
  console.log(await response.fetchingManifest)
  console.log(await response['fetchingFiles'])

  requestPackage.close()
}
