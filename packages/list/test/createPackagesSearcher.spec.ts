import createPackagesSearcher from '@pnpm/list/lib/createPackagesSearcher'

test('packages searcher', () => {
  {
    const search = createPackagesSearcher(['rimraf@*'])
    expect(search({ name: 'rimraf', version: '1.0.0' })).toBeTruthy()
    expect(search({ name: 'express', version: '1.0.0' })).not.toBeTruthy()
  }
  {
    const search = createPackagesSearcher(['rim*'])
    expect(search({ name: 'rimraf', version: '1.0.0' })).toBeTruthy()
    expect(search({ name: 'express', version: '1.0.0' })).not.toBeTruthy()
  }
  {
    const search = createPackagesSearcher(['rim*@2'])
    expect(search({ name: 'rimraf', version: '2.0.0' })).toBeTruthy()
    expect(search({ name: 'rimraf', version: '1.0.0' })).not.toBeTruthy()
  }
  {
    const search = createPackagesSearcher(['minimatch', 'once@1.4'])
    expect(search({ name: 'minimatch', version: '2.0.0' })).toBeTruthy()
    expect(search({ name: 'once', version: '1.4.1' })).toBeTruthy()
    expect(search({ name: 'rimraf', version: '1.0.0' })).not.toBeTruthy()
  }
})
