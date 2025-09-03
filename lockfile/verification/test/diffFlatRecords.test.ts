import { type Diff, diffFlatRecords } from '../src/diffFlatRecords.js'

test('diffFlatRecords', () => {
  const diff = diffFlatRecords<string, string>({
    'is-positive': '1.0.0',
    'is-negative': '2.0.0',
  }, {
    'is-negative': '2.1.0',
    'is-odd': '1.0.0',
  })
  expect(diff).toStrictEqual({
    added: [{
      key: 'is-odd',
      value: '1.0.0',
    }],
    removed: [{
      key: 'is-positive',
      value: '1.0.0',
    }],
    modified: [{
      key: 'is-negative',
      left: '2.0.0',
      right: '2.1.0',
    }],
  } as Diff<string, string>)
})
