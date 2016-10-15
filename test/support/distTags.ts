import RegClient = require('anonymous-npm-registry-client')

export async function add (pkg: string, version: string, distTag: string) {
  const client = new RegClient()

  // just to make sinopia cache the package
  await new Promise((resolve, reject) => client.distTags.fetch('http://localhost:4873', {package: pkg}, (err: Error) => err ? reject(err) : resolve()))

  // the tag has to be removed first because in sinopia it is an array of versions
  await new Promise((resolve, reject) => client.distTags.rm('http://localhost:4873', {package: pkg, distTag}, (err: Error) => err ? reject(err) : resolve()))
  await new Promise((resolve, reject) => client.distTags.add('http://localhost:4873', {package: pkg, version, distTag}, (err: Error) => err ? reject(err) : resolve()))
}
