import { getSaveType } from '../lib/getSaveType.js'

test('getSaveType()', () => {
  expect(getSaveType({ saveDev: true })).toBe('devDependencies')
  expect(getSaveType({ savePeer: true })).toBe('devDependencies')
  expect(getSaveType({ saveOptional: true })).toBe('optionalDependencies')
  expect(getSaveType({ saveProd: true })).toBe('dependencies')
  expect(getSaveType({})).toBeUndefined()
})
