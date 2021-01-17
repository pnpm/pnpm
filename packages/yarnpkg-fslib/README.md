# `@yarnpkg/fslib`

A TypeScript library abstracting the Node filesystem APIs. We use it for three main reasons:

## Type-safe paths

Our library has two path types, `NativePath` and `PortablePath`. Most interfaces only accept the later, and instances of the former need to be transformed back and forth using our type-safe utilities before being usable.

## Custom filesystems

The FSLib implements various transparent filesystem layers for a variety of purposes. For instance we use it in Yarn in order to abstract away the zip archive manipulation logic, which is implemented in `ZipFS` and exposed through a Node-like interface (called `FakeFS`).

All `FakeFS` implementations can be transparently layered on top of the builtin Node `fs` module, and that's for instance how we can add support for in-zip package loading without you having to care about the exact package format.

## Promisified API

All methods from the `FakeFS` interface are promisified by default (and suffixed for greater clarity, for instance we offer both `readFileSync` and `readFilePromise`).
