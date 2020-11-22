import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import RegClient = require('anonymous-npm-registry-client')

export async function add (pkg: string, version: string, distTag: string) {
  const client = new RegClient()

  // just to make verdaccio cache the package
  await new Promise<void>((resolve, reject) => client.distTags.fetch(`http://localhost:${REGISTRY_MOCK_PORT}`, { package: pkg }, (err: Error) => err ? reject(err) : resolve()))

  // the tag has to be removed first because in verdaccio it is an array of versions
  await new Promise<void>((resolve, reject) => client.distTags.rm(`http://localhost:${REGISTRY_MOCK_PORT}`, { package: pkg, distTag }, (err: Error) => err ? reject(err) : resolve()))
  await new Promise<void>((resolve, reject) => client.distTags.add(`http://localhost:${REGISTRY_MOCK_PORT}`, { package: pkg, version, distTag }, (err: Error) => err ? reject(err) : resolve()))
}
