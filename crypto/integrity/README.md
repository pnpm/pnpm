# @pnpm/crypto.integrity

> Parse and validate integrity strings

Parses single-hash integrity strings in the format `algorithm-base64hash` (e.g., `sha512-abc123...`) into their algorithm and hex digest components. This is the format used in pnpm lockfiles.

## Installation

```sh
pnpm add @pnpm/crypto.integrity
```

## Usage

```ts
import { parseIntegrity } from '@pnpm/crypto.integrity'

const { algorithm, hexDigest } = parseIntegrity('sha512-9/u6bgY2+JDlb7vzKD5STG+jIErimDgtYkdB0NxmODJuKCxBvl5CVNiCB3LFUYosWowMf37aGVlKfrU5RT4e1w==')

console.log(algorithm)  // 'sha512'
console.log(hexDigest)  // 'f7fbba6e0636f890e56fbbf3283e524c6fa3204ae298382d624741d0dc6638326e282c41be5e4254d8820772c5518a2c5a8c0c7f7eda19594a7eb539453e1ed7'
```

## API

### `parseIntegrity(integrity: string): ParsedIntegrity`

Parses an integrity string and returns the algorithm and hex-encoded digest.

Throws `PnpmError` with code `INVALID_INTEGRITY` if:
- The format is invalid (must be `algorithm-base64hash`)
- The base64 hash decodes to an empty digest

## License

MIT
