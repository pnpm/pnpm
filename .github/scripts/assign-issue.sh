#!/usr/bin/env bash

# Assigns an issue from an `issues` event. Reads, from the environment:
#
# - ACTION: `opened` or `edited`.
# - ISSUE_URL, AUTHOR, MAINTAINER.
# - ISSUE_BODY: the body after the event.
# - BODY_CHANGED: `true` when an edit changed the body.
# - PREVIOUS_BODY: the body before such an edit.
#
# An opened issue goes to its author when they ticked "I am working on a fix",
# and to the maintainer otherwise. Only the edit that ticks the checkbox adds
# the author, so other edits do not re-add an author a maintainer removed.

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

fix_intent() {
  "$script_dir/issue-fix-intent.sh" <<<"$1"
}

assign() {
  # GitHub refuses some authors as assignees.
  gh issue edit "$ISSUE_URL" --add-assignee "$1" ||
    echo "::warning::Could not assign the issue to $1"
}

fix_state=$(fix_intent "$ISSUE_BODY")
case "$ACTION" in
  opened)
    if [ "$fix_state" = checked ]; then
      assign "$AUTHOR"
    else
      assign "$MAINTAINER"
    fi
    ;;
  edited)
    if [ "$fix_state" = checked ] &&
      [ "$BODY_CHANGED" = true ] &&
      [ "$(fix_intent "$PREVIOUS_BODY")" != checked ]; then
      assign "$AUTHOR"
    fi
    ;;
  *)
    echo "Unexpected issue event action: $ACTION" >&2
    exit 1
    ;;
esac
