import { describe, test } from '@jest/globals'

import { isPacquetMode } from './execPnpm.js'

/**
 * Use in place of `test()` for cases that fail when the test runs against the
 * pacquet Rust port (selected via the `PNPM_E2E_BIN` env var). Pass-through
 * when running against the bundled pnpm.
 */
// `test.skip` is declared on jest's `ItBase` interface, which omits the
// `.each`, `.only`, etc. fields that `test` (`It`) carries — even though the
// runtime delegate exposes them just fine. Cast back to `typeof test` so
// callers can use `skipIfPacquet.each(...)` and friends.
export const skipIfPacquet = (isPacquetMode ? test.skip : test) as typeof test

/** describe()-level variant for skipping whole suites that aren't in pacquet's surface yet. */
export const describeSkipIfPacquet = (isPacquetMode ? describe.skip : describe) as typeof describe
