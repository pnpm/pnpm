import path = require('path')

export default function testDefaults (opts?: any): any & { store: string } { // tslint:disable-line
  return Object.assign({
    registry: 'http://localhost:4873/',
    store: path.resolve('..', '.store'),
  }, opts)
}
