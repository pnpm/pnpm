#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
stub_dir=$(mktemp -d)
trap 'rm -rf "$stub_dir"' EXIT
cat > "$stub_dir/gh" <<'STUB'
#!/usr/bin/env bash
echo "gh $*"
STUB
chmod +x "$stub_dir/gh"

checked=$'### Contributing a fix\n\n- [x] I am working on a fix'
unchecked=$'### Contributing a fix\n\n- [ ] I am working on a fix'

assert_calls() {
  local expected=$1
  local actual
  actual=$(PATH="$stub_dir:$PATH" ISSUE_URL=https://example.test/issues/1 AUTHOR=alice MAINTAINER=maintainer \
    "$script_dir/assign-issue.sh")
  if [ "$actual" != "$expected" ]; then
    printf 'for %s (body changed: %s)\nexpected:\n%s\ngot:\n%s\n' "$ACTION" "$BODY_CHANGED" "$expected" "$actual" >&2
    exit 1
  fi
}

assign_alice='gh issue edit https://example.test/issues/1 --add-assignee alice'
assign_maintainer='gh issue edit https://example.test/issues/1 --add-assignee maintainer'

export ACTION=opened BODY_CHANGED=false PREVIOUS_BODY=''
ISSUE_BODY=$checked assert_calls "$assign_alice"
ISSUE_BODY=$unchecked assert_calls "$assign_maintainer"
ISSUE_BODY='' assert_calls "$assign_maintainer"

export ACTION=edited BODY_CHANGED=true
# The edit that ticks the checkbox.
PREVIOUS_BODY=$unchecked ISSUE_BODY=$checked assert_calls "$assign_alice"
PREVIOUS_BODY='' ISSUE_BODY=$checked assert_calls "$assign_alice"
# Edits that leave the checkbox ticked, or untick it.
PREVIOUS_BODY=$checked ISSUE_BODY=$checked assert_calls ''
PREVIOUS_BODY=$checked ISSUE_BODY=$unchecked assert_calls ''
# A title-only edit of a ticked issue.
BODY_CHANGED=false PREVIOUS_BODY='' ISSUE_BODY=$checked assert_calls ''

if PATH="$stub_dir:$PATH" ACTION=closed ISSUE_BODY='' "$script_dir/assign-issue.sh" > /dev/null 2>&1; then
  echo 'expected an unknown action to fail' >&2
  exit 1
fi
