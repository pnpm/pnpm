import pLimit from 'p-limit'

export const limitLinking = pLimit(16)
export const limitModulesDirReads = pLimit(16)
