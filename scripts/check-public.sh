#!/bin/sh
# Refuses a commit that would put private material in this public repo: an employer's name or
# systems, a work email, a personal home path, or a private repo or link. The words to refuse
# live outside the repo, one regular expression per line, in ~/.config/claude-kanban-dsh/deny.txt
# (so the list itself stays private); a few generic ones are built in.
#
# Install once per clone:  git config core.hooksPath scripts/hooks
# Run by hand:             sh scripts/check-public.sh [--all]
#   (no argument: the staged changes; --all: every tracked file)

deny_file="${KANBAN_DENY_FILE:-$HOME/.config/claude-kanban-dsh/deny.txt}"
builtin='C:[\/]+Users[\/]+[A-Za-z0-9._-]+[\/]|/mnt/c/Users/[A-Za-z0-9._-]+/|claude\.ai/(code/)?artifact/'
pattern="$builtin"
if [ -f "$deny_file" ]; then
  extra=$(grep -vE '^\s*(#|$)' "$deny_file" | tr -d '\r' | paste -sd '|' -)
  [ -n "$extra" ] && pattern="$pattern|$extra"
fi

# Test data may use made-up home folders (C:\Users\me, C:\Users\h); list allowed lines here.
allow='(C:[\/]+Users[\/]+|/mnt/c/Users/)(me|h|u|x|o.neil|someone[a-z-]*|you)[\/]'

if [ "$1" = "--all" ]; then
  hits=$(git grep -nIiE "$pattern" -- . ':!scripts/check-public.sh' | grep -viE "$allow")
else
  hits=$(git diff --cached -U0 --no-color -- . ':!scripts/check-public.sh' \
    | grep -E '^(\+\+\+ |\+[^+])' \
    | awk '/^\+\+\+ /{f=substr($0,7); next} {print f": "substr($0,2)}' \
    | grep -iE "$pattern" | grep -viE "$allow")
fi

if [ -n "$hits" ]; then
  echo "check-public: refused. These lines look private (see scripts/check-public.sh):" >&2
  echo "$hits" | cut -c1-200 >&2
  exit 1
fi
exit 0
