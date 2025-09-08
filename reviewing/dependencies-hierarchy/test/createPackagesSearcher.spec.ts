import {DependencyManifest} from '@pnpm/types'
import { createPackagesSearcher } from '../lib/createPackagesSearcher.js'

test('packages searcher', () => {
  {
    const search = createPackagesSearcher(['rimraf@*'])
    expect(search(mockContext({ name: 'rimraf', version: '1.0.0' }))).toBeTruthy()
    expect(search(mockContext({ name: 'express', version: '1.0.0' }))).not.toBeTruthy()
  }
  {
    const search = createPackagesSearcher(['rim*'])
    expect(search(mockContext({ name: 'rimraf', version: '1.0.0' }))).toBeTruthy()
    expect(search(mockContext({ name: 'express', version: '1.0.0' }))).not.toBeTruthy()
  }
  {
    const search = createPackagesSearcher(['rim*@2'])
    expect(search(mockContext({ name: 'rimraf', version: '2.0.0' }))).toBeTruthy()
    expect(search(mockContext({ name: 'rimraf', version: '1.0.0' }))).not.toBeTruthy()
  }
  {
    const search = createPackagesSearcher(['minimatch', 'once@1.4'])
    expect(search(mockContext({ name: 'minimatch', version: '2.0.0' }))).toBeTruthy()
    expect(search(mockContext({ name: 'once', version: '1.4.1' }))).toBeTruthy()
    expect(search(mockContext({ name: 'rimraf', version: '1.0.0' }))).not.toBeTruthy()
  }
})

function mockContext (manifest: DependencyManifest) {
  return {
    name: manifest.name,
    version: manifest.version,
    readManifest: () => manifest,
  }
}
