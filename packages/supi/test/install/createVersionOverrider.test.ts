import createVersionsOverrider from 'supi/lib/install/createVersionsOverrider'

test('createVersionsOverrider()', () => {
  const overrider = createVersionsOverrider({
    'foo@2': '2.12.0',
  })
  expect(
    overrider({
      dependencies: { foo: '^2.10.0' },
    })
  ).toStrictEqual({ dependencies: { foo: '2.12.0' } })
})
