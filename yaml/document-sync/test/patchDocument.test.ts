import { patchDocument } from '@pnpm/yaml.document-sync'
import yaml from 'yaml'

describe('patchNode', () => {
  it('throws error when document has errors', () => {
    const raw = `\
foo:
  bar: 1
  - 2
`
    const document = yaml.parseDocument(raw)

    expect(() => {
      patchDocument(document, {})
    }).toThrow('Document with errors cannot be patched')
  })

  it('throws error when encountering unknown node at top-level', () => {
    const raw = `\
- 1
- 2
`

    const document = yaml.parseDocument(raw);

    // Inserting a raw value that should definitely be wrong.
    (document.contents as unknown as number) = 3

    expect(() => {
      patchDocument(document, [1, 2, 3])
    }).toThrow('Unrecognized yaml node')
  })

  it('empties document when target is null', () => {
    const raw = `\
- 1
- 2
- 3
`
    const document = yaml.parseDocument(raw)
    patchDocument(document, null)

    expect(document.contents).toBeNull()
  })
})

describe('scalar', () => {
  it('updates nested scalar', () => {
    const raw = `\
foo:
  bar:
    # This comment on baz should be preserved when changing it.
    baz: 1
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    json.foo.bar.baz = 2

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  bar:
    # This comment on baz should be preserved when changing it.
    baz: 2
`)
  })

  it('changes from a scalar to a different type', () => {
    const raw = `\
foo:
  bar:
    # This comment on baz should be preserved when changing it.
    baz: 1
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    json.foo.bar.baz = [1, 2]

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  bar:
    # This comment on baz should be preserved when changing it.
    baz:
      - 1
      - 2
`)
  })
})

it('does not reformat string quotes', () => {
  const raw = `\
foo:
  bar:
    baz: [ '1', "2" ]
  qux:
    - "1"
    - '2'
`

  const document = yaml.parseDocument(raw)
  const json = document.toJSON()

  json.foo.quux = 3

  patchDocument(document, json)

  expect(document.toString()).toBe(`\
foo:
  bar:
    baz: [ '1', "2" ]
  qux:
    - "1"
    - '2'
  quux: 3
`)
})

describe('map', () => {
  it('adds new items to a map and preserves comment', () => {
    const raw = `\
foo:
  bar:
    # Comment 1
    baz: 1
  # Comment 2
  qux: 2
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    json.foo.quux = 3

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  bar:
    # Comment 1
    baz: 1
  # Comment 2
  qux: 2
  quux: 3
`)
  })

  it('adds new items to a map and handles comment immediately after map definition', () => {
    const raw = `\
items:
  # Comment on items in map
  b: 2
  # Comment on d
  d: 4
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, { items: { a: 1, b: 2, c: 3, d: 4 } })

    // The yaml library unfortunately parses the first comment as a property on
    // the "items" map rather than a property on the "b" field. So the newly
    // added "a" field is added below the comment.
    //
    // This isn't incorrect, but most of the time users probably associate the
    // comment as a part of the immediately succeeding field. Let's encode the
    // behavior as a test for now. The behavior may change in a future version
    // of the yaml library.
    expect(document.toString()).toBe(`\
items:
  # Comment on items in map
  a: 1
  b: 2
  c: 3
  # Comment on d
  d: 4
`)
  })

  it('removes item from map and preserves comment', () => {
    const raw = `\
foo:
  bar:
    # Comment 1
    baz: 1
  # Comment 2
  qux: 2
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    delete json.foo.bar.baz

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  # Comment 2
  qux: 2
`)
  })

  it('changes from a map to a different type', () => {
    const raw = `\
foo:
  bar:
    baz: 1
  qux: 2
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    // Change foo.bar to be a list instead.
    json.foo.bar = [1, 2, 3]

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  bar:
    - 1
    - 2
    - 3
  qux: 2
`)
  })

  it('uses key order from target map', () => {
    const raw = `\
# a
a: 1
# b
b: 2
# c
c: 3
# d
d: 4
# e
e: 5
`

    const document = yaml.parseDocument(raw)

    const target = {
      d: 4,
      c: 3,
      a: 1,
      e: 5,
      b: 2,
    }

    patchDocument(document, target)

    expect(document.toString()).toBe(`\
# d
d: 4
# c
c: 3
# a
a: 1
# e
e: 5
# b
b: 2
`)
  })

  it('throws error when encountering unknown key node in map', () => {
    const raw = `\
foo: 1
  `

    const document = yaml.parseDocument(raw)
    const contents = document.contents as yaml.YAMLMap

    // The key here should also be wrapped around a scalar constructor.
    contents.items.push(new yaml.Pair('bar', new yaml.Scalar('2')))

    expect(() => {
      patchDocument(document, { foo: 1, bar: 2 })
    }).toThrow('Encountered unexpected non-node value: bar')
  })

  it('throws error when encountering unknown value node in map', () => {
    const raw = `\
foo: 1
  `

    const document = yaml.parseDocument(raw)
    const contents = document.contents as yaml.YAMLMap

    // The value here should also be wrapped around a scalar constructor.
    contents.items.push(new yaml.Pair(new yaml.Scalar('bar'), 2))

    expect(() => {
      patchDocument(document, { foo: 1, bar: 2 })
    }).toThrow('Encountered unexpected non-node value: 2')
  })
})

describe('list', () => {
  it('adds new items to a list and preserves comment', () => {
    const raw = `\
foo:
  bar:
    baz:
      - 1
      # Comment
      - 2
      - 3
  qux: 2
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    json.foo.bar.baz.push(4)

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  bar:
    baz:
      - 1
      # Comment
      - 2
      - 3
      - 4
  qux: 2
`)
  })

  it('removes items from a list along with its comment', () => {
    const raw = `\
list:
  - 1
  # Comment
  - 2
  - 3
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    delete json.list[1]

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
list:
  - 1
  - 3
`)
  })

  it('removes items from a list but preserves comments below', () => {
    const raw = `\
- 1
- 2
# Comment on 3
- 3
# Comment on 4
- 4
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [1, 3, 4])

    expect(document.toString()).toBe(`\
- 1
# Comment on 3
- 3
# Comment on 4
- 4
`)
  })

  it('updates items in a list that contain duplicates', () => {
    const raw = `\
# Comment on first instance of 1
- 1
- 2
# Comment on second instance of 1
- 1
# Comment on 4
- 4
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [1, 3, 4, 1])

    expect(document.toString()).toBe(`\
# Comment on first instance of 1
- 1
- 3
# Comment on 4
- 4
# Comment on second instance of 1
- 1
`)
  })

  // Similar to the test above, but make sure the presence of a complex object
  // doesn't cause the list reconciler to fall back a different code path that
  // won't handle primitives efficiently.
  it('removes items from a list but preserves comments below when source list has complex object', () => {
    const raw = `\
- 1
- {}
- 2
# Comment on 3
- 3
# Comment on 4
- 4
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [1, 3, 4])

    expect(document.toString()).toBe(`\
- 1
# Comment on 3
- 3
# Comment on 4
- 4
`)
  })

  it('updates items in a complex list', () => {
    const raw = `\
# Comment on foo
- foo: 1
# Comment on qux
- qux: 2
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [{ foo: 1 }, { bar: 2 }, { qux: 3 }])

    // It's unfortunately very difficult (and inherently ambiguous) to keep the
    // comment on qux in the right place. This is because the complex list item
    // reconciler is index based and doesn't know qux shifted down one element.
    //
    // It's especially difficult to tell where the comment on qux should be when
    // its value changes too like in this example (qux: 2 -> 3).
    expect(document.toString()).toBe(`\
# Comment on foo
- foo: 1
# Comment on qux
- bar: 2
- qux: 3
`)
  })

  it('updates items in primitive list with holes', () => {
    const raw = `\
- 1
- 2
# Comment on 3
- 3
# Comment on 4
- 4
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [1, 3, null, undefined, 4])

    expect(document.toString()).toBe(`\
- 1
# Comment on 3
- 3
# Comment on 4
- 4
`)
  })

  it('updates items in complex list with holes', () => {
    const raw = `\
- foo: 1
- 2
- 3
- 4
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [{ foo: 1 }, 3, null, undefined, 4, 5])

    expect(document.toString()).toBe(`\
- foo: 1
- 3
- 4
- 5
`)
  })

  // This may not be the desired behavior in every case. It's inherently
  // ambiguous and depends on whether the comment written applies to the newly
  // added item.
  it('changes item in list and removes comment', () => {
    const raw = `\
- 1
# Comment on 2
- 2
- 5
`

    const document = yaml.parseDocument(raw)

    patchDocument(document, [1, 3, 4, 5])

    expect(document.toString()).toBe(`\
- 1
- 3
- 4
- 5
`)
  })

  it('changes from a list to a different type', () => {
    const raw = `\
foo:
  bar:
    - 1
    - 2
  qux: 2
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    json.foo.bar = { baz: 1 }

    patchDocument(document, json)

    expect(document.toString()).toBe(`\
foo:
  bar:
    baz: 1
  qux: 2
`)
  })

  it('throws error when encountering unknown node in primitive list', () => {
    const raw = `\
- 1
- 2
`

    const document = yaml.parseDocument(raw)
    const contents = document.contents as yaml.YAMLSeq

    // The correct way to modify the AST would be:
    //
    //   content.items.push(new yaml.Scalar(3))
    //
    // Inserting the raw raw value should cause the patch function to throw.
    contents.items.push(3)

    expect(() => {
      patchDocument(document, [1, 2, 3])
    }).toThrow('Encountered unexpected non-node value: 3')
  })

  it('throws error when encountering unknown node in complex list', () => {
    const raw = `\
- foo: 1
- bar: 2
`

    const document = yaml.parseDocument(raw)
    const contents = document.contents as yaml.YAMLSeq

    // The correct way to modify the AST would be:
    //
    //   content.items.push(new yaml.Scalar(3))
    //
    // Inserting the raw raw value should cause the patch function to throw.
    contents.items.push({ qux: 3 })

    expect(() => {
      patchDocument(document, [{ foo: 1 }, { bar: 2 }, { qux: 3 }])
    }).toThrow('Encountered unexpected non-node value: [object Object]')
  })
})

describe('alias', () => {
  it('updates aliases in original location when alias=follow', () => {
    const raw = `\
foo: &config
  - 1
  - 2

bar: *config
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    // When aliases are used, the toJSON function will reuse the same object. We
    // have to create a new list to get a representative test.
    json.bar = [...json.bar, 3]

    patchDocument(document, json, { aliases: 'follow' })

    expect(document.toString()).toBe(`\
foo: &config
  - 1
  - 2
  - 3

bar: *config
`)
  })

  it('removes alias when alias=unwrap', () => {
    const raw = `\
foo: &config
  - 1
  - 2

bar: *config
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    // When aliases are used, the toJSON function will reuse the same object. We
    // have to create a new list to get a representative test.
    json.bar = [...json.bar, 3]

    patchDocument(document, json, { aliases: 'unwrap' })

    expect(document.toString()).toBe(`\
foo: &config
  - 1
  - 2

bar:
  - 1
  - 2
  - 3
`)
  })

  it('updates anchor nodes when alias=follow', () => {
    const raw = `\
foo: &config
  - 1
  - 2

bar: *config
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    // When aliases are used, the toJSON function will reuse the same object. We
    // have to create a new list to get a representative test.
    json.bar = [...json.bar, 3]

    patchDocument(document, json, { aliases: 'follow' })

    expect(document.toString()).toBe(`\
foo: &config
  - 1
  - 2
  - 3

bar: *config
`)
  })

  it('alias unwraps correctly when modifying anchor node', () => {
    const raw = `\
foo: &config
  - 1
  - 2

bar: *config
`

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    // When aliases are used, the toJSON function will reuse the same object. We
    // have to create a new list to get a representative test.
    json.foo = [...json.foo, 3]

    patchDocument(document, json, { aliases: 'unwrap' })

    expect(document.toString()).toBe(`\
foo: &config
  - 1
  - 2
  - 3

bar:
  - 1
  - 2
`)
  })

  // It's not completely clear what to do in this case. The library uses the value of the last encounter.
  it('updates anchor and alias nodes with conflicting values when alias=follow', () => {
    const raw = `\
foo: &config
  - 1
  - 2

bar: *config
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    // When aliases are used, the toJSON function will reuse the same object. We
    // have to create a new list to get a representative test.
    json.foo = [...json.foo, 3]
    json.bar = [...json.bar, 4]

    patchDocument(document, json, { aliases: 'follow' })

    expect(document.toString()).toBe(`\
foo: &config
  - 1
  - 2
  - 4

bar: *config
`)
  })

  it('throws explicit error when encountering unresolved alias', () => {
    const raw = `\
foo: &config
  - 1
  - 2

bar: *config
  `

    const document = yaml.parseDocument(raw)
    const json = document.toJSON()

    const contents = document.contents as yaml.YAMLMap
    const foo = contents.get('foo') as yaml.YAMLSeq
    foo.anchor = undefined

    // When aliases are used, the toJSON function will reuse the same object. We
    // have to create a new list to get a representative test.
    json.bar = [...json.bar, 3]

    expect(() => {
      patchDocument(document, json)
    }).toThrow('Failed to resolve yaml alias: config')
  })
})
