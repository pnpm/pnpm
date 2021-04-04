import createVersionsOverrider from 'supi/lib/install/createVersionsOverrider'

test('createVersionsOverrider() matches subranges', () => {
  const overrider = createVersionsOverrider({
    'foo@2': '2.12.0',
  })
  expect(
    overrider({
      dependencies: { foo: '^2.10.0' },
    })
  ).toStrictEqual({ dependencies: { foo: '2.12.0' } })
})

test('createVersionsOverrider() does not fail on non-range selectors', () => {
  const overrider = createVersionsOverrider({
    'foo@2': '2.12.0',
    'bar@github:org/bar': '2.12.0',
  })
  expect(
    overrider({
      dependencies: {
        foo: 'github:org/foo',
        bar: 'github:org/bar',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: 'github:org/foo',
      bar: '2.12.0',
    },
  })
})
