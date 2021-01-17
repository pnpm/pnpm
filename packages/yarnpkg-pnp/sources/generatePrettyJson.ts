enum PrettyJsonState {
  DEFAULT = `DEFAULT`,
  TOP_LEVEL = `TOP_LEVEL`,
  FALLBACK_EXCLUSION_LIST = `FALLBACK_EXCLUSION_LIST`,
  FALLBACK_EXCLUSION_ENTRIES = `FALLBACK_EXCLUSION_ENTRIES`,
  FALLBACK_EXCLUSION_DATA = `FALLBACK_EXCLUSION_DATA`,
  PACKAGE_REGISTRY_DATA = `PACKAGE_REGISTRY_DATA`,
  PACKAGE_REGISTRY_ENTRIES = `PACKAGE_REGISTRY_ENTRIES`,
  PACKAGE_STORE_DATA = `PACKAGE_STORE_DATA`,
  PACKAGE_STORE_ENTRIES = `PACKAGE_STORE_ENTRIES`,
  PACKAGE_INFORMATION_DATA = `PACKAGE_INFORMATION_DATA`,
  PACKAGE_DEPENDENCIES = `PACKAGE_DEPENDENCIES`,
  PACKAGE_DEPENDENCY = `PACKAGE_DEPENDENCY`,
}

type PrettyJsonMachine = {
  [key: string]: {
    collapsed: boolean,
    next: {
      [key: string]: PrettyJsonState,
      [`*`]: PrettyJsonState,
    },
  },
};

const prettyJsonMachine: PrettyJsonMachine = {
  [PrettyJsonState.DEFAULT]: {
    collapsed: false,
    next: {
      [`*`]: PrettyJsonState.DEFAULT,
    },
  },
  // {
  //   "fallbackExclusionList": ...
  // }
  [PrettyJsonState.TOP_LEVEL]: {
    collapsed: false,
    next: {
      [`fallbackExclusionList`]: PrettyJsonState.FALLBACK_EXCLUSION_LIST,
      [`packageRegistryData`]: PrettyJsonState.PACKAGE_REGISTRY_DATA,
      [`*`]: PrettyJsonState.DEFAULT,
    },
  },
  // "fallbackExclusionList": [
  //   ...
  // ]
  [PrettyJsonState.FALLBACK_EXCLUSION_LIST]: {
    collapsed: false,
    next: {
      [`*`]: PrettyJsonState.FALLBACK_EXCLUSION_ENTRIES,
    },
  },
  // "fallbackExclusionList": [
  //   [...]
  // ]
  [PrettyJsonState.FALLBACK_EXCLUSION_ENTRIES]: {
    collapsed: true,
    next: {
      [`*`]: PrettyJsonState.FALLBACK_EXCLUSION_DATA,
    },
  },
  // "fallbackExclusionList": [
  //   [..., [...]]
  // ]
  [PrettyJsonState.FALLBACK_EXCLUSION_DATA]: {
    collapsed: true,
    next: {
      [`*`]: PrettyJsonState.DEFAULT,
    },
  },
  // "packageRegistryData": [
  //   ...
  // ]
  [PrettyJsonState.PACKAGE_REGISTRY_DATA]: {
    collapsed: false,
    next: {
      [`*`]: PrettyJsonState.PACKAGE_REGISTRY_ENTRIES,
    },
  },
  // "packageRegistryData": [
  //   [...]
  // ]
  [PrettyJsonState.PACKAGE_REGISTRY_ENTRIES]: {
    collapsed: true,
    next: {
      [`*`]: PrettyJsonState.PACKAGE_STORE_DATA,
    },
  },
  // "packageRegistryData": [
  //   [..., [
  //     ...
  //   ]]
  // ]
  [PrettyJsonState.PACKAGE_STORE_DATA]: {
    collapsed: false,
    next: {
      [`*`]: PrettyJsonState.PACKAGE_STORE_ENTRIES,
    },
  },
  // "packageRegistryData": [
  //   [..., [
  //     [...]
  //   ]]
  // ]
  [PrettyJsonState.PACKAGE_STORE_ENTRIES]: {
    collapsed: true,
    next: {
      [`*`]: PrettyJsonState.PACKAGE_INFORMATION_DATA,
    },
  },
  // "packageRegistryData": [
  //   [..., [
  //     [..., {
  //       ...
  //     }]
  //   ]]
  // ]
  [PrettyJsonState.PACKAGE_INFORMATION_DATA]: {
    collapsed: false,
    next: {
      [`packageDependencies`]: PrettyJsonState.PACKAGE_DEPENDENCIES,
      [`*`]: PrettyJsonState.DEFAULT,
    },
  },
  // "packageRegistryData": [
  //   [..., [
  //     [..., {
  //       "packagePeers": [
  //         ...
  //       ]
  //     }]
  //   ]]
  // ]
  [PrettyJsonState.PACKAGE_DEPENDENCIES]: {
    collapsed: false,
    next: {
      [`*`]: PrettyJsonState.PACKAGE_DEPENDENCY,
    },
  },
  // "packageRegistryData": [
  //   [..., [
  //     [..., {
  //       "packageDependencies": [
  //         [...]
  //       ]
  //     }]
  //   ]]
  // ]
  [PrettyJsonState.PACKAGE_DEPENDENCY]: {
    collapsed: true,
    next: {
      [`*`]: PrettyJsonState.DEFAULT,
    },
  },
};

function generateCollapsedArray(data: Array<any>, state: PrettyJsonState, indent: string) {
  let result = ``;

  result += `[`;

  for (let t = 0, T = data.length; t < T; ++t) {
    result += generateNext(String(t), data[t], state, indent).replace(/^ +/g, ``);
    if (t + 1 < T) {
      result += `, `;
    }
  }

  result += `]`;

  return result;
}

function generateExpandedArray(data: Array<any>, state: PrettyJsonState, indent: string) {
  const nextIndent = `${indent}  `;

  let result = ``;

  result += indent;
  result += `[\n`;

  for (let t = 0, T = data.length; t < T; ++t) {
    result += nextIndent + generateNext(String(t), data[t], state, nextIndent).replace(/^ +/, ``);

    if (t + 1 < T)
      result += `,`;

    result += `\n`;
  }

  result += indent;
  result += `]`;

  return result;
}

function generateCollapsedObject(data: {[key: string]: any}, state: PrettyJsonState, indent: string) {
  const keys = Object.keys(data);

  let result = ``;

  result += `{`;

  for (let t = 0, T = keys.length; t < T; ++t) {
    const key = keys[t];
    const value = data[key];

    if (typeof value === `undefined`)
      continue;

    result += JSON.stringify(key);
    result += `: `;
    result += generateNext(key, value, state, indent).replace(/^ +/g, ``);
    if (t + 1 < T) {
      result += `, `;
    }
  }

  result += `}`;

  return result;
}

function generateExpandedObject(data: {[key: string]: any}, state: PrettyJsonState, indent: string) {
  const keys = Object.keys(data);
  const nextIndent = `${indent}  `;

  let result = ``;

  result += indent;
  result += `{\n`;

  for (let t = 0, T = keys.length; t < T; ++t) {
    const key = keys[t];
    const value = data[key];

    if (typeof value === `undefined`)
      continue;

    result += nextIndent;
    result += JSON.stringify(key);
    result += `: `;
    result += generateNext(key, value, state, nextIndent).replace(/^ +/g, ``);

    if (t + 1 < T)
      result += `,`;

    result += `\n`;
  }

  result += indent;
  result += `}`;

  return result;
}

function generateNext(key: string, data: any, state: PrettyJsonState, indent: string) {
  const {next} = prettyJsonMachine[state];
  const nextState = next[key] || next[`*`];

  return generate(data, nextState, indent);
}

function generate(data: any, state: PrettyJsonState, indent: string) {
  const {collapsed} = prettyJsonMachine[state];

  if (Array.isArray(data)) {
    if (collapsed) {
      return generateCollapsedArray(data, state, indent);
    } else {
      return generateExpandedArray(data, state, indent);
    }
  }

  if (typeof data === `object` && data !== null) {
    if (collapsed) {
      return generateCollapsedObject(data, state, indent);
    } else {
      return generateExpandedObject(data, state, indent);
    }
  }

  return JSON.stringify(data);
}

export function generatePrettyJson(data: any) {
  return generate(data, PrettyJsonState.TOP_LEVEL, ``);
}
