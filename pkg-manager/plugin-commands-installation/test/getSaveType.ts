import { getSaveType } from '../lib/getSaveType.js'

test('getSaveType()', () => {
  expect(getSaveType({ saveDev: true })).toEqual('devDependencies')
  expect(getSaveType({ savePeer: true })).toEqual('devDependencies')
  expect(getSaveType({ saveOptional: true })).toEqual('optionalDependencies')
  expect(getSaveType({ saveProd: true })).toEqual('dependencies')
  expect(getSaveType({})).toBeUndefined()
})
