#!/bin/sh
# A stand-in for Claude Code in the macOS smoke test (scripts/smoke-macos.sh, card #117). The
# smoke test copies it to $HOME/smoke-bin/claude (and, for launch 1, $HOME/.local/bin/claude)
# in a throwaway HOME. It acts enough like claude for the app's checks:
#
#   --version            prints "9.9.9 (Claude Code)", so the app picks it as the newest claude
#   otherwise            takes --session-id ID or --resume ID and --settings FILE as the app
#                        passes them, then:
#     - writes $HOME/fake/<id>.env: PATH, LANG, ulimit -n, its terminal, and what `yes | head`
#       printed (a "Broken pipe" there means SIGPIPE was ignored in the session);
#     - posts the SessionStart hook to the URL in the settings file;
#     - runs the statusLine command once (`kb status`) with a minimal status JSON, which is
#       what marks a session ready in the app;
#     - starts `nohup sleep 600 &` from a shell that exits at once, so launchd adopts the sleep
#       before the app's 5-second process tracker sees it; its pid goes to <id>.sleep;
#     - reads what the app types: the first line to <id>.first, every line to <id>.in;
#     - on SIGHUP (the terminal closed) writes <id>.hup and exits 129.
# It writes <id>.pid and card-<KANBAN_CARD>.id so the smoke test can find each card's files.
# It never records the settings file, the hook URLs or KANBAN_API: they hold the app's token.

case "${1:-}" in
    --version)
        echo "9.9.9 (Claude Code)"
        exit 0
        ;;
esac

ID=""
SETTINGS=""
while [ $# -gt 0 ]; do
    case "$1" in
        --session-id | --resume)
            [ $# -ge 2 ] || break
            ID=$2
            shift 2
            ;;
        --settings)
            [ $# -ge 2 ] || break
            SETTINGS=$2
            shift 2
            ;;
        --model | --effort | --permission-mode | --mcp-config | --agents)
            [ $# -ge 2 ] || break
            shift 2
            ;;
        *) shift ;;
    esac
done
[ -n "$ID" ] || ID="noid-$$"

F="$HOME/fake"
mkdir -p "$F"
trap 'echo hup >"$F/$ID.hup"; exit 129' HUP
trap 'echo term >"$F/$ID.term"; exit 143' TERM

echo "$$" >"$F/$ID.pid"
echo "$ID" >"$F/card-${KANBAN_CARD:-none}.id"

# Like Claude's TUI, read the terminal raw: no echo, and no 1024-byte line limit (a long
# typed message would stall a canonical-mode read).
stty -icanon -echo min 1 time 0 2>/dev/null || true

{
    echo "PATH=$PATH"
    echo "LANG=${LANG:-}"
    echo "LC_ALL=${LC_ALL:-}"
    echo "SHELL=${SHELL:-}"
    echo "ULIMIT_N=$(ulimit -n)"
    echo "TTY=$(tty 2>&1)"
    echo "YES_HEAD=$( (yes | head -n 1) 2>&1 | tr '\n' ' ')"
    echo "CWD=$(pwd)"
} >"$F/$ID.env.tmp"
mv -f "$F/$ID.env.tmp" "$F/$ID.env"

# One value from the settings file: plutil reads JSON; sed is the fallback.
setting() {
    plutil -extract "$1" raw -o - "$SETTINGS" 2>/dev/null
}

if [ -n "$SETTINGS" ] && [ -f "$SETTINGS" ]; then
    URL=$(setting hooks.SessionStart.0.hooks.0.url) ||
        URL=$(sed -n 's/.*"\(http:[^"]*\/SessionStart\)".*/\1/p' "$SETTINGS" | head -n 1)
    if [ -n "$URL" ]; then
        if curl -s -m 5 -o /dev/null -X POST -H 'Content-Type: application/json' \
            -d "{\"hook_event_name\":\"SessionStart\",\"session_id\":\"$ID\",\"source\":\"startup\"}" \
            "$URL"; then
            echo ok >"$F/$ID.hook"
        else
            echo "curl failed" >"$F/$ID.hook"
        fi
    else
        echo "no SessionStart hook in the settings" >"$F/$ID.hook"
    fi

    CMD=$(setting statusLine.command) ||
        CMD=$(sed -n 's/.*"command" *: *"\(\\"[^"]*\\" status\)".*/\1/p' "$SETTINGS" | head -n 1 | sed 's/\\"/"/g')
    if [ -n "$CMD" ]; then
        printf '{"session_id":"%s","model":{"id":"fake","display_name":"Fake"},"context_window":{"used_percentage":1}}' "$ID" |
            sh -c "$CMD" >"$F/$ID.status" 2>&1 || true
    else
        echo "no statusLine command in the settings" >"$F/$ID.status"
    fi
fi

# The leftover a real session leaves: a backgrounded process whose shell has already gone.
sh -c 'nohup sleep 600 >/dev/null 2>&1 & echo $! >"$1"' sh "$F/$ID.sleep"

n=0
while IFS= read -r line; do
    if [ "$n" -eq 0 ]; then
        printf '%s\n' "$line" >"$F/$ID.first"
    fi
    printf '%s\n' "$line" >>"$F/$ID.in"
    n=$((n + 1))
done

# The terminal reached end of input: stay alive (as claude does) until hung up, for at most
# 30 minutes so nothing is left behind if the app never ends it.
i=0
while [ "$i" -lt 1800 ]; do
    sleep 1
    i=$((i + 1))
done
exit 0
