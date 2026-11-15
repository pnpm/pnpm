## 1100.2.6

### Patch Changes

- Linking bins is faster for packages that ship large native executables as bins. pnpm now reads only the first 512 bytes of a bin file to find its shebang [#17361](https://github.com/pnpm/pnpm/issues/17361).
