import path = require('path')

export default function testDefaults (opts?: any): any & { storeDir: string } { // tslint:disable-line
  return Object.assign({
    registry: 'http://localhost:4873/',
    storeDir: path.resolve('..', '.store'),
  }, opts)
}
