#!/usr/bin/env bash
# Checks that the installed Flatpak reads, writes and gets change notifications
# from the host's dconf database. Run on the device as the desktop user after
# installing the bundle. Uses cursor-blink-timeout, which has no visible effect,
# and puts it back.

set -u
APP=net.dunevent.NightManager
KEY=(org.gnome.desktop.interface cursor-blink-timeout)
FP=(flatpak run --command=gsettings "$APP")
PASS=0
FAIL=0

check() {
  if [[ $2 == "$3" ]]; then
    PASS=$((PASS + 1)); echo "PASS  $1"
  else
    FAIL=$((FAIL + 1)); echo "FAIL  $1 (expected $2, got $3)"
  fi
}

flatpak info "$APP" >/dev/null 2>&1 || { echo "$APP isn't installed." >&2; exit 1; }
orig=$(gsettings get "${KEY[@]}")
new=$((orig + 3))
trap 'gsettings set "${KEY[@]}" "$orig"' EXIT

echo "GIO modules in the sandbox:"
flatpak run --command=sh "$APP" -c 'ls /app/lib/gio/modules 2>&1; echo "GSETTINGS_BACKEND=$GSETTINGS_BACKEND"'

check "read" "$orig" "$("${FP[@]}" get "${KEY[@]}" 2>&1 | tail -n1)"

out=$(mktemp)
# script(1) gives gsettings a pty, so each line is flushed before the kill.
script -qfc "timeout 30 $(printf '%q ' "${FP[@]}") monitor ${KEY[*]}" "$out" >/dev/null 2>&1 &
mon=$!
sleep 8
gsettings set "${KEY[@]}" "$new"
sleep 3
kill "$mon" 2>/dev/null
wait "$mon" 2>/dev/null
grep -q "cursor-blink-timeout: $new" "$out" && got=yes || got=no
check "change notification" yes "$got"
rm -f "$out"

gsettings set "${KEY[@]}" "$orig"
"${FP[@]}" set "${KEY[@]}" "$new" 2>&1
sleep 1
check "write" "$new" "$(gsettings get "${KEY[@]}")"

echo
echo "$PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
