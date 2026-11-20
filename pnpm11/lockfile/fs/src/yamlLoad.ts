import { CORE_SCHEMA, load, loadAll, mergeTag } from 'js-yaml'

/** Lockfiles may use YAML merge keys (`<<`), which js-yaml's default schema does not read. */
const LOCKFILE_SCHEMA = CORE_SCHEMA.withTags(mergeTag)

export function loadLockfileYaml (content: string): unknown {
  return load(content, { schema: LOCKFILE_SCHEMA })
}

/** Like {@link loadLockfileYaml}, but a document with no content reads as `undefined` instead of throwing. */
export function loadLockfileYamlDocument (content: string): unknown {
  return loadAll(content, { schema: LOCKFILE_SCHEMA })[0]
}
