import { describe, it } from 'node:test'

import { RuleTester } from 'eslint'
import tseslint from 'typescript-eslint'

import perfectionist from '../perfectionist/index.js'

RuleTester.describe = describe
RuleTester.it = it
RuleTester.itOnly = it.only

const ruleTester = new RuleTester({
  languageOptions: { parser: tseslint.parser },
})

const { rules } = perfectionist

ruleTester.run('excessive-nesting', rules['excessive-nesting'], {
  valid: [
    { code: 'function f () { if (a) { for (const x of y) { if (x) {} } } }', options: [{ maxDepth: 3 }] },
    { code: 'function f () { if (a) {} else if (b) {} else if (c) { if (d) { while (e) {} } } }', options: [{ maxDepth: 3 }] },
    // A nested function declaration is measured on its own.
    { code: 'function f () { if (a) { if (b) { function g () { if (c) { if (d) { if (e) {} } } } } } }', options: [{ maxDepth: 3 }] },
    // A test callback plays the role of a test function.
    { code: 'describe("x", () => { test("y", () => { if (a) { if (b) { if (c) {} } } }) })', options: [{ maxDepth: 3 }] },
    { code: 'const f = (a) => a ? b : c ? d : e', options: [{ maxDepth: 1 }] },
  ],
  invalid: [
    {
      code: 'function f () { if (a) { for (const x of y) { if (x) { while (z) {} } } } }',
      options: [{ maxDepth: 3 }],
      errors: [{ messageId: 'tooDeep', data: { name: 'function `f`', depth: 4, maxDepth: 3 } }],
    },
    {
      // A closure is a level of the body that contains it.
      code: 'async function f () { await Promise.all(xs.map(async (x) => { try { if (x) {} } catch {} })) }',
      options: [{ maxDepth: 2 }],
      errors: [{ messageId: 'tooDeep', data: { name: 'function `f`', depth: 3, maxDepth: 2 } }],
    },
    {
      code: 'const f = () => { { if (a) {} } }',
      options: [{ maxDepth: 1 }],
      errors: [{ messageId: 'tooDeep', data: { name: 'function `f`', depth: 2, maxDepth: 1 } }],
    },
  ],
})

ruleTester.run('excessive-cognitive-complexity', rules['excessive-cognitive-complexity'], {
  valid: [
    // if (1) + else if (1) + else (1)
    { code: 'function f () { if (a) {} else if (b) {} else {} }', options: [{ maxComplexity: 3 }] },
    // one run of `&&` (1) plus the `||` run (1)
    { code: 'const f = () => a && b && c || d', options: [{ maxComplexity: 2 }] },
    // A nested function declaration is measured on its own.
    { code: 'function f () { if (c) {} function g () { if (a) { if (b) {} } } }', options: [{ maxComplexity: 3 }] },
  ],
  invalid: [
    {
      // for (1) + nested if (2) + nested ternary in a closure (4)
      code: 'function f () { for (const x of y) { if (x) { run(() => x ? 1 : 2) } } }',
      options: [{ maxComplexity: 6 }],
      errors: [{ messageId: 'tooComplex', data: { name: 'function `f`', complexity: 7, maxComplexity: 6 } }],
    },
    {
      // if (1) + recursion (1) + labelled continue inside a loop (2 + 1)
      code: 'function f (n) { if (n) f(n - 1); outer: for (;;) { continue outer } }',
      options: [{ maxComplexity: 3 }],
      errors: [{ messageId: 'tooComplex', data: { name: 'function `f`', complexity: 4, maxComplexity: 3 } }],
    },
    {
      // try adds nothing; catch (1) + nested if (2)
      code: 'function f () { try {} catch (err) { if (err) {} } }',
      options: [{ maxComplexity: 2 }],
      errors: [{ messageId: 'tooComplex', data: { name: 'function `f`', complexity: 3, maxComplexity: 2 } }],
    },
  ],
})

ruleTester.run('overly-complex-condition', rules['overly-complex-condition'], {
  valid: [
    { code: 'if (a && b || c) {}', options: [{ maxOperators: 2 }] },
    { code: 'if (xs.some((x) => x.a && x.b && x.c)) {}', options: [{ maxOperators: 0 }] },
    { code: 'if (a ?? b) {}', options: [{ maxOperators: 0 }] },
  ],
  invalid: [
    {
      code: 'while (a && b || c && !(d || e)) {}',
      options: [{ maxOperators: 3 }],
      errors: [{ messageId: 'tooComplex', data: { count: 4, maxOperators: 3 } }],
    },
    {
      code: 'const x = a && f(b || c) ? 1 : 2',
      options: [{ maxOperators: 1 }],
      errors: [{ messageId: 'tooComplex', data: { count: 2, maxOperators: 1 } }],
    },
  ],
})

ruleTester.run('too-many-local-bindings', rules['too-many-local-bindings'], {
  valid: [
    { code: 'function f (a, b, c) { let x = 1; x = 2; let _y; const x2 = x }', options: [{ maxBindings: 2 }] },
    { code: 'function f () { const a = 1; function g () { const b = 1; const c = 2 } }', options: [{ maxBindings: 2 }] },
  ],
  invalid: [
    {
      code: 'function f () { const { a, b: [c] } = x; try {} catch (err) {} xs.map((item) => item) }',
      options: [{ maxBindings: 3 }],
      errors: [{ messageId: 'tooMany', data: { name: 'function `f`', count: 4, maxBindings: 3 } }],
    },
  ],
})

ruleTester.run('overly-long-function', rules['overly-long-function'], {
  valid: [
    { code: 'function f () {\n  a()\n\n  // comment\n  b()\n}', options: [{ maxLines: 2 }] },
  ],
  invalid: [
    {
      code: 'function f () {\n  a()\n  b()\n  c()\n}',
      options: [{ maxLines: 2 }],
      errors: [{ messageId: 'tooLong', data: { name: 'function `f`', count: 3, maxLines: 2 } }],
    },
  ],
})

ruleTester.run('overly-long-method-chain', rules['overly-long-method-chain'], {
  valid: [
    { code: 'a.b().c().d()', options: [{ maxCalls: 3 }] },
    { code: 'f(a.b().c()).d().e()', options: [{ maxCalls: 2 }] },
  ],
  invalid: [
    {
      code: 'a.b()!.c?.().d().e()',
      options: [{ maxCalls: 3 }],
      errors: [{ messageId: 'tooLong', data: { count: 4, maxCalls: 3 } }],
    },
  ],
})

ruleTester.run('single-letter-let-binding', rules['single-letter-let-binding'], {
  valid: ['const n = 1', 'const count = 1', 'try {} catch (err) {}'],
  invalid: [
    { code: 'const x = 1', errors: [{ messageId: 'singleLetter', data: { name: 'x' } }] },
    { code: 'const { a } = obj', errors: [{ messageId: 'singleLetter', data: { name: 'a' } }] },
    { code: 'try {} catch (e) {}', errors: [{ messageId: 'singleLetter', data: { name: 'e' } }] },
  ],
})

ruleTester.run('single-letter-function-param', rules['single-letter-function-param'], {
  valid: ['function f (i, n) {}', 'class A { m (value) {} }'],
  invalid: [
    { code: 'function f (x) {}', errors: [{ messageId: 'singleLetter', data: { name: 'x' } }] },
  ],
})

ruleTester.run('single-letter-closure-param', rules['single-letter-closure-param'], {
  valid: [
    'function f () { xs.map((x) => x.name) }',
    'function f () { xs.filter((x) => isOk(x)) }',
    'function f () { xs.sort((a, b) => a.localeCompare(b) || b - a) }',
    'function f () { xs.map((i) => i * 2) }',
  ],
  invalid: [
    {
      code: 'function f () { xs.map((x) => x * 2) }',
      errors: [{ messageId: 'singleLetter', data: { name: 'x' } }],
    },
    {
      code: 'function f () { xs.forEach((x) => { log(x); save(x) }) }',
      errors: [{ messageId: 'singleLetter', data: { name: 'x' } }],
    },
  ],
})

ruleTester.run('single-letter-generic', rules['single-letter-generic'], {
  valid: ['function f<Value> (value: Value) {}'],
  invalid: [
    { code: 'function f<T> (value: T) {}', errors: [{ messageId: 'singleLetter', data: { name: 'T' } }] },
  ],
})

ruleTester.run('bare-issue-reference', rules['bare-issue-reference'], {
  valid: [
    '// See pnpm/pnpm#123',
    '// See https://github.com/pnpm/pnpm/issues/123#issuecomment-1',
    '// The `#1` token',
  ],
  invalid: [
    { code: '// Fixes #123', errors: [{ messageId: 'finding', data: { text: '#123' } }] },
  ],
})

ruleTester.run('unicode-ellipsis-in-comments', rules['unicode-ellipsis-in-comments'], {
  valid: ['// wait...'],
  invalid: [{ code: '// wait…', errors: [{ messageId: 'finding' }] }],
})

ruleTester.run('unpinned-repo-ref', rules['unpinned-repo-ref'], {
  valid: [
    '// https://github.com/nodejs/node/blob/v22.0.0/lib/fs.js',
    '// https://github.com/nodejs/node/blob/0123abcd/lib/fs.js',
    '// https://github.com/pnpm/pnpm/issues/1',
    'const url = "https://github.com/pnpm/pnpm/blob/main/README.md"',
  ],
  invalid: [
    {
      code: '// https://github.com/pnpm/pnpm/blob/main/README.md',
      errors: [{ messageId: 'finding', data: { ref: 'main' } }],
    },
  ],
})

ruleTester.run('allow-directives-without-reason', rules['allow-directives-without-reason'], {
  valid: [
    '// eslint-disable-next-line no-console -- the CLI prints its result',
    '/* eslint-disable no-console -- the CLI prints its result */',
  ],
  invalid: [
    { code: '// eslint-disable-next-line no-console\nconsole.log(1)', errors: [{ messageId: 'missingReason' }] },
  ],
})
