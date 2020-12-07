import complete from '../src/cmd/complete'

test('complete an option value', async () => {
  const completions = await complete(
    {
      cliOptionsTypesByCommandName: {
        install: () => ({
          'resolution-strategy': ['fast', 'fewer-dependencies'],
        }),
      },
      completionByCommandName: {},
      initialCompletion: () => [],
      shorthandsByCommandName: {},
      universalOptionsTypes: {},
    },
    {
      cmd: 'install',
      currentTypedWordType: null,
      lastOption: '--resolution-strategy',
      options: {},
      params: [],
    }
  )
  expect(completions).toStrictEqual([
    { name: 'fast' },
    { name: 'fewer-dependencies' },
  ])
})

test('complete a command', async () => {
  const ctx = {
    cliOptionsTypesByCommandName: {
      run: () => ({
        'if-present': Boolean,
      }),
    },
    completionByCommandName: {
      run: async () => [{ name: 'test' }],
    },
    initialCompletion: () => [],
    shorthandsByCommandName: {},
    universalOptionsTypes: {
      filter: String,
    },
  }
  expect(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    [{ name: 'test' }]
  )
  expect(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: null,
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    [
      { name: 'test' },
      { name: '--filter' },
      { name: '--if-present' },
      { name: '--no-if-present' },
    ]
  )
  expect(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    [
      { name: '--filter' },
      { name: '--if-present' },
      { name: '--no-if-present' },
    ]
  )
})

test('if command completion fails, return empty array', async () => {
  expect(
    await complete(
      {
        cliOptionsTypesByCommandName: {},
        completionByCommandName: {
          run: async () => {
            throw new Error('error')
          },
        },
        initialCompletion: () => [],
        shorthandsByCommandName: {},
        universalOptionsTypes: {
          filter: String,
        },
      },
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    []
  )
})

test('initial completion', async () => {
  const ctx = {
    cliOptionsTypesByCommandName: {},
    completionByCommandName: {},
    initialCompletion: () => [
      { name: 'add' },
      { name: 'install' },
    ],
    shorthandsByCommandName: {},
    universalOptionsTypes: {
      filter: String,
    },
  }
  expect(
    await complete(ctx,
      {
        cmd: null,
        currentTypedWordType: null,
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual([
    { name: 'add' },
    { name: 'install' },
    { name: '--filter' },
    { name: '--version' },
  ])
  expect(
    await complete(ctx,
      {
        cmd: 'ad',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual([
    { name: 'add' },
    { name: 'install' },
  ])
  expect(
    await complete(ctx,
      {
        cmd: null,
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual([
    { name: '--filter' },
    { name: '--version' },
  ])
})

test('suggest no completions for after --version', async () => {
  expect(
    await complete(
      {
        cliOptionsTypesByCommandName: {},
        completionByCommandName: {},
        initialCompletion: () => [
          { name: 'add' },
          { name: 'install' },
        ],
        shorthandsByCommandName: {},
        universalOptionsTypes: {},
      },
      {
        cmd: null,
        currentTypedWordType: null,
        lastOption: null,
        options: { version: true },
        params: [],
      }
    )
  ).toStrictEqual([])
})
