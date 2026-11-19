#!/usr/bin/env bash
# Runs nightmanager-daemon on a private session bus with its own dconf database
# and checks what it does to the host settings.
#
#   cargo build && tests/daemon.sh

set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(dirname "$HERE")
DAEMON="$ROOT/target/debug/nightmanager-daemon"
APP=net.dunevent.NightManager
DEST=(--dest net.dunevent.NightManager.Daemon --object-path /net/dunevent/NightManager/Daemon)
IFACE=net.dunevent.NightManager.Daemon

PASS=0
FAIL=0
PIDS=()
DAEMON_PID=""

WORK=$(mktemp -d)
mkdir -p "$WORK/schemas"
cp "$ROOT"/data/*.gschema.xml "$ROOT"/data/schemas/*.gschema.xml "$WORK/schemas/"
glib-compile-schemas "$WORK/schemas" || exit 1
read -ra GIO_FLAGS <<<"$(pkg-config --cflags --libs gio-2.0)"
gcc -o "$WORK/fake-screensaver" "$HERE/fake-screensaver.c" "${GIO_FLAGS[@]}" || exit 1
gcc -o "$WORK/fake-services" "$HERE/fake-services.c" "${GIO_FLAGS[@]}" || exit 1

cleanup() {
  for p in "${PIDS[@]}" $DAEMON_PID; do kill "$p" 2>/dev/null; done
  wait 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

# new_env [bare]: "bare" leaves out the stand-ins for gsd, feedbackd and the light
# sensor, like a device without those features.
new_env() {
  local d
  d=$(mktemp -d -p "$WORK")
  export HOME="$d/home" XDG_CONFIG_HOME="$d/config" XDG_STATE_HOME="$d/state" XDG_RUNTIME_DIR="$d/run"
  export GSETTINGS_SCHEMA_DIR="$WORK/schemas" GSETTINGS_BACKEND=dconf
  mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"
  local out
  out=$(dbus-daemon --session --fork --print-address=1 --print-pid=1)
  DBUS_SESSION_BUS_ADDRESS=$(sed -n 1p <<<"$out")
  export DBUS_SESSION_BUS_ADDRESS
  PIDS+=("$(sed -n 2p <<<"$out")")
  out=$(dbus-daemon --session --fork --print-address=1 --print-pid=1)
  DBUS_SYSTEM_BUS_ADDRESS=$(sed -n 1p <<<"$out")
  export DBUS_SYSTEM_BUS_ADDRESS
  PIDS+=("$(sed -n 2p <<<"$out")")
  if [[ ${1:-} != bare ]]; then
    "$WORK/fake-services" org.gnome.SettingsDaemon.Color org.gnome.SettingsDaemon.Power \
      org.sigxcpu.Feedback & PIDS+=("$!")
    "$WORK/fake-services" --system net.hadess.SensorProxy & PIDS+=("$!")
    sleep 0.3
  fi
  host org.gnome.desktop.interface color-scheme "'default'"
  host org.gnome.desktop.interface gtk-theme "'DayTheme'"
  host org.gnome.settings-daemon.plugins.color night-light-enabled false
  host org.gnome.settings-daemon.plugins.power ambient-enabled false
  host org.sigxcpu.feedbackd profile "'full'"
  # No schedule unless a test sets one.
  app evening-times "['', '', '', '', '', '', '']"
  app morning-times "['', '', '', '', '', '', '']"
}

app() { gsettings set $APP "$1" "$2"; }
host() { gsettings set "$1" "$2" "$3"; }
get() { gsettings get "$1" "$2"; }

start_daemon() {
  RUST_LOG=debug "$DAEMON" >>"$WORK/daemon.log" 2>&1 &
  DAEMON_PID=$!
  for _ in $(seq 1 50); do
    gdbus introspect --session "${DEST[@]}" >/dev/null 2>&1 && break
    sleep 0.1
  done
  sleep 0.3
}

stop_daemon() { kill "$DAEMON_PID" 2>/dev/null; wait "$DAEMON_PID" 2>/dev/null; DAEMON_PID=""; }
call() { gdbus call --session "${DEST[@]}" --method "$IFACE.$1" >/dev/null; sleep 0.4; }
prop() {
  gdbus call --session "${DEST[@]}" --method org.freedesktop.DBus.Properties.Get "$IFACE" "$1" \
    | sed -E 's/^\(<(.*)>,\)$/\1/'
}

check() {
  local desc=$1 expected=$2 got=$3
  if [[ $got == "$expected" ]]; then
    PASS=$((PASS + 1)); printf 'PASS  %s\n' "$desc"
  else
    FAIL=$((FAIL + 1)); printf 'FAIL  %s\n      expected: %s\n      got:      %s\n' "$desc" "$expected" "$got"
  fi
}

# An `as` value with only the weekday of now+<offset> set, to that time.
slot() {
  local when="$1" day time list=()
  day=$(( $(date -d "$when" +%u) - 1 ))
  time=$(date -d "$when" +%H:%M)
  for i in 0 1 2 3 4 5 6; do
    if [[ $i -eq $day ]]; then list+=("'$time'"); else list+=("''"); fi
  done
  local IFS=,
  echo "[${list[*]}]"
}

echo "-- schedule start and end"
new_env; start_daemon
app evening-times "$(slot '-10 min')"; sleep 0.8
check "night starts at a past evening boundary" "true" "$(prop Active)"
check "dark style" "'prefer-dark'" "$(get org.gnome.desktop.interface color-scheme)"
check "night light" "true" "$(get org.gnome.settings-daemon.plugins.color night-light-enabled)"
check "automatic brightness" "true" "$(get org.gnome.settings-daemon.plugins.power ambient-enabled)"
check "feedback silent" "'silent'" "$(get org.sigxcpu.feedbackd profile)"
app morning-times "$(slot '-1 min')"; sleep 0.8
check "night ends at a later morning boundary" "false" "$(prop Active)"
check "dark style restored" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
check "night light restored" "false" "$(get org.gnome.settings-daemon.plugins.color night-light-enabled)"
check "brightness restored" "false" "$(get org.gnome.settings-daemon.plugins.power ambient-enabled)"
check "feedback restored" "'full'" "$(get org.sigxcpu.feedbackd profile)"
call StartNight; call StartNight
check "starting twice keeps the first day values" "true" "$(prop Active)"
call EndNight
check "and restores them" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
stop_daemon

echo "-- manual triggers"
new_env; start_daemon
app trigger-dark-style true; sleep 0.4
call StartNight
host org.gnome.desktop.interface color-scheme "'prefer-light'"; sleep 0.5
check "leaving dark style ends night mode" "false" "$(prop Active)"
check "other settings restored" "'full'" "$(get org.sigxcpu.feedbackd profile)"
host org.gnome.desktop.interface color-scheme "'prefer-dark'"; sleep 0.5
check "choosing dark style starts night mode" "true" "$(prop Active)"
call EndNight
app trigger-dark-style false
app trigger-feedback "['quiet', 'silent']"; sleep 0.4
host org.sigxcpu.feedbackd profile "'quiet'"; sleep 0.5
check "Not Full trigger starts on quiet" "true" "$(prop Active)"
check "night level applied" "'silent'" "$(get org.sigxcpu.feedbackd profile)"
host org.sigxcpu.feedbackd profile "'quiet'"; sleep 0.5
check "move within the group keeps night mode" "true" "$(prop Active)"
host org.sigxcpu.feedbackd profile "'full'"; sleep 0.5
check "Full ends night mode" "false" "$(prop Active)"
stop_daemon

echo "-- theme changes at night"
new_env; start_daemon
app gtk-theme "'NightTheme'"; app save-theme-changes true; sleep 0.4
call StartNight
check "night theme applied" "'NightTheme'" "$(get org.gnome.desktop.interface gtk-theme)"
host org.gnome.desktop.interface gtk-theme "'Picked'"; sleep 0.5
check "user's night pick saved" "'Picked'" "$(get $APP gtk-theme)"
call EndNight
check "day theme restored" "'DayTheme'" "$(get org.gnome.desktop.interface gtk-theme)"
app save-theme-changes false; sleep 0.3
call StartNight
host org.gnome.desktop.interface gtk-theme "'Other'"; sleep 0.5
check "not saved when the option is off" "'Picked'" "$(get $APP gtk-theme)"
call EndNight
stop_daemon

echo "-- configuration changes during the night"
new_env; start_daemon
call StartNight
app dark-style false; sleep 0.6
check "dropped behavior restored at once" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
check "night mode still on" "true" "$(prop Active)"
app gtk-theme "'Late'"; sleep 0.6
check "new behavior applied at once" "'Late'" "$(get org.gnome.desktop.interface gtk-theme)"
call EndNight
check "and restored with the rest" "'DayTheme'" "$(get org.gnome.desktop.interface gtk-theme)"
stop_daemon

echo "-- restart keeps the night"
new_env; start_daemon
call StartNight; stop_daemon; start_daemon
check "active after restart" "true" "$(prop Active)"
call EndNight
check "day values restored after restart" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
stop_daemon

echo "-- Night Light waits for the screen"
new_env
"$WORK/fake-screensaver" active & PIDS+=("$!"); sleep 0.5
app screen-off-mode "'wait'"; start_daemon
call StartNight
check "other settings applied with the screen off" "'prefer-dark'" "$(get org.gnome.desktop.interface color-scheme)"
check "Night Light held while off" "false" "$(get org.gnome.settings-daemon.plugins.color night-light-enabled)"
gdbus call --session --dest org.gnome.ScreenSaver --object-path /org/gnome/ScreenSaver \
  --method test.Control.SetActive false >/dev/null; sleep 0.5
check "Night Light applied when the screen turns on" "true" "$(get org.gnome.settings-daemon.plugins.color night-light-enabled)"
stop_daemon

echo "-- old package present"
new_env
mkdir -p "$HOME/.config/systemd/user/timers.target.wants" "$HOME/.config/nightmanager" \
  "$HOME/.config/systemd/user/nightmanager-on.timer.d" "$HOME/.config/systemd/user/nightmanager-off.timer.d" \
  "$HOME/.local/state/nightmanager"
touch "$HOME/unit"
ln -s "$HOME/unit" "$HOME/.config/systemd/user/timers.target.wants/nightmanager-on.timer"
printf 'NLT_ON=0\nBRT_ON=1\nDRK_ON=1\nLEG_ON="adw-gtk3-dark"\nACT_ON="0"\nICN_ON="0"\nFB_LEV=2\n' \
  >"$HOME/.config/nightmanager/nightmanager.conf"
printf '[Timer]\nOnCalendar=\nOnCalendar=*-*-* %s:00\n' "$(date -d '-3 min' +%H:%M)" \
  >"$HOME/.config/systemd/user/nightmanager-on.timer.d/10-override.conf"
printf '[Timer]\nOnCalendar=\nOnCalendar=*-*-* %s:00\n' "$(date -d '-1 min' +%H:%M)" \
  >"$HOME/.config/systemd/user/nightmanager-off.timer.d/10-override.conf"
printf "NLT_STATE=0\nBRT_STATE=0\nDRK_STATE='prefer-light'\nLEG_STATE='OldDay'\nACT_STATE=0\nICN_STATE=0\nFBK_STATE='full'\n" \
  >"$HOME/.local/state/nightmanager/restore.conf"
start_daemon
check "blocked while installed" "true" "$(prop LegacyInstalled)"
check "config imported: night light off" "false" "$(get $APP night-light)"
check "config imported: GTK3 theme" "'adw-gtk3-dark'" "$(get $APP gtk-theme)"
check "config imported: feedback quiet" "'quiet'" "$(get $APP feedback-level)"
check "config imported: times" "'$(date -d '-3 min' +%H:%M)'" "$(gsettings get $APP evening-times | tr -d '[]' | cut -d, -f1)"
call StartNight
check "nothing changes while blocked" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
check_legacy() { gdbus call --session "${DEST[@]}" --method "$IFACE.CheckLegacy"; sleep 0.4; }
check "still installed" "(true,)" "$(check_legacy)"
rm "$HOME/.config/systemd/user/timers.target.wants/nightmanager-on.timer"
check "removed" "(false,)" "$(check_legacy)"
check "unblocked" "false" "$(prop LegacyInstalled)"
check "old night ended with its saved day values" "'prefer-light'" "$(get org.gnome.desktop.interface color-scheme)"
check "old saved theme restored" "'OldDay'" "$(get org.gnome.desktop.interface gtk-theme)"
check "night mode off per imported schedule" "false" "$(prop Active)"
stop_daemon

echo "-- features the device lacks"
new_env bare; start_daemon
call StartNight
check "night light left alone" "false" "$(get org.gnome.settings-daemon.plugins.color night-light-enabled)"
check "brightness left alone" "false" "$(get org.gnome.settings-daemon.plugins.power ambient-enabled)"
check "feedback left alone" "'full'" "$(get org.sigxcpu.feedbackd profile)"
check "dark style still applied" "'prefer-dark'" "$(get org.gnome.desktop.interface color-scheme)"
call EndNight
check "day restored" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
stop_daemon

echo "-- no ambient light sensor"
FAKE_NO_AMBIENT_LIGHT=1 new_env; start_daemon
call StartNight
check "brightness left alone without a sensor" "false" "$(get org.gnome.settings-daemon.plugins.power ambient-enabled)"
check "night light still applied" "true" "$(get org.gnome.settings-daemon.plugins.color night-light-enabled)"
call EndNight
stop_daemon

echo "-- daytime settings"
daytime() {
  gdbus call --session "${DEST[@]}" --method "$IFACE.SetDaytime" "$1" "$2" 2>&1 | tr -d '\n'
  sleep 0.3
}
new_env; start_daemon
check "day: applied now" "(false,)" "$(daytime icon-theme "<'DayIcons'>")"
check "day: host changed" "'DayIcons'" "$(get org.gnome.desktop.interface icon-theme)"
call StartNight
gdbus monitor --session "${DEST[@]:0:2}" >"$WORK/monitor.log" 2>&1 & MON=$!
sleep 0.3
check "night: managed key waits" "(true,)" "$(daytime color-scheme "<'prefer-light'>")"
kill "$MON" 2>/dev/null; wait "$MON" 2>/dev/null
check "saved day value announced" "yes" "$(grep -q DaytimeChanged "$WORK/monitor.log" && echo yes || echo no)"
check "night: host keeps night value" "'prefer-dark'" "$(get org.gnome.desktop.interface color-scheme)"
check "night: saved value listed" "yes" "$(gdbus call --session "${DEST[@]}" --method "$IFACE.GetDaytime" \
  | grep -qF "('color-scheme', <'prefer-light'>, true)" && echo yes || echo no)"
check "night: unmanaged key applied now" "(false,)" "$(daytime gtk-theme "<'OtherTheme'>")"
check "night: unmanaged host changed" "'OtherTheme'" "$(get org.gnome.desktop.interface gtk-theme)"
check "missing key refused" "yes" "$(daytime nope "<'x'>" | grep -q InvalidArgs && echo yes || echo no)"
check "wrong type refused" "yes" "$(daytime color-scheme "<true>" | grep -q InvalidArgs && echo yes || echo no)"
call EndNight
check "morning uses the new day value" "'prefer-light'" "$(get org.gnome.desktop.interface color-scheme)"
check "unmanaged key kept" "'OtherTheme'" "$(get org.gnome.desktop.interface gtk-theme)"
stop_daemon

echo "-- wallpaper and lock screen"
BG=org.gnome.desktop.background
new_env
host $BG picture-uri "'file:///day.png'"
host $BG picture-uri-dark "'file:///day-dark.png'"
host $BG primary-color "'#123456'"
host org.gnome.desktop.screensaver picture-uri "'file:///day-lock.png'"
app wallpaper "'image'"; app wallpaper-image "'file:///night.png'"; app lock-screen "'blank'"
start_daemon
call StartNight
check "night picture" "'file:///night.png'" "$(get $BG picture-uri)"
check "night picture in dark style too" "'file:///night.png'" "$(get $BG picture-uri-dark)"
check "color left alone for a picture" "'#123456'" "$(get $BG primary-color)"
check "lock screen picture cleared" "''" "$(get org.gnome.desktop.screensaver picture-uri)"
call EndNight
check "day picture restored" "'file:///day.png'" "$(get $BG picture-uri)"
check "day dark picture restored" "'file:///day-dark.png'" "$(get $BG picture-uri-dark)"
check "day lock screen restored" "'file:///day-lock.png'" "$(get org.gnome.desktop.screensaver picture-uri)"
app wallpaper "'blank'"; sleep 0.4
call StartNight
check "no picture" "''" "$(get $BG picture-uri)"
check "no dark picture" "''" "$(get $BG picture-uri-dark)"
check "color untouched" "'#123456'" "$(get $BG primary-color)"
call EndNight
check "day picture back after no picture" "'file:///day.png'" "$(get $BG picture-uri)"
app wallpaper "'image'"; app wallpaper-image "''"; sleep 0.4
call StartNight
check "picture mode without a picture changes nothing" "'file:///day.png'" "$(get $BG picture-uri)"
call EndNight
stop_daemon

echo "-- quit"
new_env; start_daemon
call StartNight
call Quit
# An exited child stays a zombie until reaped, so the state is read from ps.
sleep 0.5
check "daemon exited" "gone" "$(ps -o stat= -p "$DAEMON_PID" | grep -qv Z && echo running || echo gone)"
wait "$DAEMON_PID" 2>/dev/null; DAEMON_PID=""
start_daemon
check "night still active after restart" "true" "$(prop Active)"
call EndNight
check "day restored after restart" "'default'" "$(get org.gnome.desktop.interface color-scheme)"
stop_daemon

echo
echo "== $PASS passed, $FAIL failed"
if [[ $FAIL -gt 0 ]]; then tail -n 40 "$WORK/daemon.log"; fi
[[ $FAIL -eq 0 ]]
