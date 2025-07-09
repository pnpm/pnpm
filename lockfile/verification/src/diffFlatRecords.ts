export interface AddedRemoved<Key, Value> {
  key: Key
  value: Value
}

export interface Modified<Key, Value> {
  key: Key
  left: Value
  right: Value
}

export interface Diff<Key, Value> {
  /** Entries that are absent in `left` but present in `right`. */
  added: Array<AddedRemoved<Key, Value>>
  /** Entries that are present in `left` but absent in `right`. */
  removed: Array<AddedRemoved<Key, Value>>
  /** Entries that are present in both `left` and `right` but the values are different. */
  modified: Array<Modified<Key, Value>>
}

export function diffFlatRecords<Key extends string | number | symbol, Value> (left: Record<Key, Value>, right: Record<Key, Value>): Diff<Key, Value> {
  const result: Diff<Key, Value> = {
    added: [],
    removed: [],
    modified: [],
  }

  for (const [key, value] of Object.entries(left) as Array<[Key, Value]>) {
    if (!Object.hasOwn(right, key)) {
      result.removed.push({ key, value })
    } else if (value !== right[key]) {
      result.modified.push({ key, left: value, right: right[key] })
    }
  }

  for (const [key, value] of Object.entries(right) as Array<[Key, Value]>) {
    if (!Object.hasOwn(left, key)) {
      result.added.push({ key, value })
    }
  }

  return result
}

export function isEqual ({ added, removed, modified }: Diff<unknown, unknown>): boolean {
  return added.length === 0 && removed.length === 0 && modified.length === 0
}
