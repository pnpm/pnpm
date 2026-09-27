#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

assert_intent() {
  local expected=$1
  local body=$2
  local actual
  actual=$("$script_dir/issue-fix-intent.sh" <<< "$body")
  if [ "$actual" != "$expected" ]; then
    printf 'expected %s, got %s for body:\n%s\n' "$expected" "$actual" "$body" >&2
    exit 1
  fi
}

assert_intent checked $'### Describe the Bug\n\nIt breaks.\n\n### Contributing a fix\n\n- [x] I am working on a fix'
assert_intent checked $'### Contributing a fix\r\n\r\n- [X] I am working on a fix\r'
assert_intent unchecked $'### Describe the Bug\n\nIt breaks.\n\n### Contributing a fix\n\n- [ ] I am working on a fix'
assert_intent missing $'### Describe the Bug\n\nIt breaks.\n\nA fix is prepared and will be submitted as a PR shortly.'
assert_intent missing $'### Describe the Bug\n\n- [x] I am working on a fix'
assert_intent missing $'### Contributing a fix\n\nI will send a PR.'
assert_intent missing ''
