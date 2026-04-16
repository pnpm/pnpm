/**
 * Parse an uncompressed /v1/files payload and write each file to the CAFS.
 * Returns the number of files newly written (EEXIST is not counted).
 */
export function writeFiles (storeDir: string, payload: Buffer): number
