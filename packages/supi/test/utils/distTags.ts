import RegClient = require('anonymous-npm-registry-client')

export async function add (packageName: string, version: string, distTag: string) {
  const client = new RegClient()

  // just to make verdaccio cache the package
  await new Promise((resolve, reject) => client.distTags.fetch('http://localhost:4873', { package: packageName }, (err: Error) => err ? reject(err) : resolve()))

  // the tag has to be removed first because in verdaccio it is an array of versions
  await new Promise((resolve, reject) => client.distTags.rm('http://localhost:4873', { package: packageName, distTag }, (err: Error) => err ? reject(err) : resolve()))
  await new Promise((resolve, reject) => client.distTags.add('http://localhost:4873', { package: packageName, version, distTag }, (err: Error) => err ? reject(err) : resolve()))
}
