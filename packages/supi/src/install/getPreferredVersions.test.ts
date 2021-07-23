import { getAllUniqueSpecs } from './getPreferredVersions'

test('getAllUniqueSpecs()', () => {
  expect(getAllUniqueSpecs([
    {
      name: '',
      version: '',
      dependencies: {
        foo: '1.0.0',
        bar: '1.0.0',
        qar: '1.0.0',
        zoo: 'link:../zoo',
        alpha: 'npm:beta@2',
      },
    },
    {
      name: '',
      version: '',
      dependencies: {
        bar: '1.0.0',
        qar: '2.0.0',
      },
    },
  ])).toStrictEqual({
    foo: '1.0.0',
    bar: '1.0.0',
  })
})
