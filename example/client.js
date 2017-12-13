'use strict'
const connectStoreController = require('@pnpm/server').connectStoreController

main()
  .then(() => console.log('Done'))
  .catch(err => console.error(err))

 async function main () {
  const port = 5813
  const hostname = '127.0.0.1'
  const registry = 'https://registry.npmjs.org/'
  const prefix = process.cwd()
  const storeCtrl = await connectStoreController({port, hostname})
  const response = await storeCtrl.requestPackage(
    {alias: 'is-positive', pref: '1.0.0'},
    {
      downloadPriority: 0,
      loggedPkg: {rawSpec: 'sfdf'},
      offline: false,
      prefix,
      registry,
      verifyStoreIntegrity: false,
    }
  )

  console.log(response)
  console.log(await response.fetchingManifest)
  console.log(await response['fetchingFiles'])

  await storeCtrl.updateConnections(prefix, {addDependencies: [response.id], removeDependencies: []})
  await storeCtrl.saveState()

  await storeCtrl.close()
}
