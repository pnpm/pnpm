import { expect, test } from '@jest/globals'

import { type Identifier, parseIdentifier } from '../../src/index.js'

test('not an identifier', () => {
  expect(parseIdentifier('')).toBeUndefined()
  expect(parseIdentifier('-')).toBeUndefined()
  expect(parseIdentifier('-foo')).toBeUndefined()
  expect(parseIdentifier('+a')).toBeUndefined()
  expect(parseIdentifier('7z')).toBeUndefined()
})

test('identifier only', () => {
  expect(parseIdentifier('_')).toStrictEqual([{
    type: 'identifier',
    content: '_',
  } as Identifier, ''])
  expect(parseIdentifier('a')).toStrictEqual([{
    type: 'identifier',
    content: 'a',
  } as Identifier, ''])
  expect(parseIdentifier('abc')).toStrictEqual([{
    type: 'identifier',
    content: 'abc',
  } as Identifier, ''])
  expect(parseIdentifier('helloWorld')).toStrictEqual([{
    type: 'identifier',
    content: 'helloWorld',
  } as Identifier, ''])
  expect(parseIdentifier('HelloWorld')).toStrictEqual([{
    type: 'identifier',
    content: 'HelloWorld',
  } as Identifier, ''])
  expect(parseIdentifier('a123')).toStrictEqual([{
    type: 'identifier',
    content: 'a123',
  } as Identifier, ''])
  expect(parseIdentifier('abc123')).toStrictEqual([{
    type: 'identifier',
    content: 'abc123',
  } as Identifier, ''])
  expect(parseIdentifier('helloWorld123')).toStrictEqual([{
    type: 'identifier',
    content: 'helloWorld123',
  } as Identifier, ''])
  expect(parseIdentifier('HelloWorld123')).toStrictEqual([{
    type: 'identifier',
    content: 'HelloWorld123',
  } as Identifier, ''])
  expect(parseIdentifier('hello_world_123')).toStrictEqual([{
    type: 'identifier',
    content: 'hello_world_123',
  } as Identifier, ''])
  expect(parseIdentifier('__abc_123__')).toStrictEqual([{
    type: 'identifier',
    content: '__abc_123__',
  } as Identifier, ''])
  expect(parseIdentifier('_0')).toStrictEqual([{
    type: 'identifier',
    content: '_0',
  } as Identifier, ''])
  expect(parseIdentifier('_foo')).toStrictEqual([{
    type: 'identifier',
    content: '_foo',
  } as Identifier, ''])
  expect(parseIdentifier('some-package-name')).toStrictEqual([{
    type: 'identifier',
    content: 'some-package-name',
  } as Identifier, ''])
  expect(parseIdentifier('helloWorld123-456')).toStrictEqual([{
    type: 'identifier',
    content: 'helloWorld123-456',
  } as Identifier, ''])
  expect(parseIdentifier('_under-score')).toStrictEqual([{
    type: 'identifier',
    content: '_under-score',
  } as Identifier, ''])
  expect(parseIdentifier('foo--bar')).toStrictEqual([{
    type: 'identifier',
    content: 'foo--bar',
  } as Identifier, ''])
  expect(parseIdentifier('foo-')).toStrictEqual([{
    type: 'identifier',
    content: 'foo-',
  } as Identifier, ''])
})

test('identifier and tail', () => {
  expect(parseIdentifier('a+b')).toStrictEqual([{
    type: 'identifier',
    content: 'a',
  } as Identifier, '+b'])
  expect(parseIdentifier('abc.def')).toStrictEqual([{
    type: 'identifier',
    content: 'abc',
  } as Identifier, '.def'])
  expect(parseIdentifier('foo-bar.baz')).toStrictEqual([{
    type: 'identifier',
    content: 'foo-bar',
  } as Identifier, '.baz'])
  expect(parseIdentifier('foo-bar[0]')).toStrictEqual([{
    type: 'identifier',
    content: 'foo-bar',
  } as Identifier, '[0]'])
  expect(parseIdentifier('HelloWorld123 456')).toStrictEqual([{
    type: 'identifier',
    content: 'HelloWorld123',
  } as Identifier, ' 456'])
  expect(parseIdentifier('hello_world_123 456')).toStrictEqual([{
    type: 'identifier',
    content: 'hello_world_123',
  } as Identifier, ' 456'])
  expect(parseIdentifier('__abc_123__++__def_456__')).toStrictEqual([{
    type: 'identifier',
    content: '__abc_123__',
  } as Identifier, '++__def_456__'])
})
