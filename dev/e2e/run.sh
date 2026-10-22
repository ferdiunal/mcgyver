#!/bin/bash
# End-to-end test in a nested, headless KWin: a GTK app drags a file, KWin's fake input moves
# the pointer and presses modifiers, spectacle takes screenshots.
#   dev/e2e/run.sh [scenario]     (default: full) → logs and screenshots in dev/e2e/out/
#   SOURCE=dolphin dev/e2e/run.sh …   drags from Dolphin instead of the GTK test window
#   FILE=photos.zip dev/e2e/run.sh archive   drags another file from out/files (real conversion)
#   SOURCE=dolphin dev/e2e/run.sh tap   taps Shift with a file selected in Dolphin, no drag
#   PERMISSIONS=1 SETUP=… dev/e2e/run.sh permissions   KWin checks permissions; set up from the settings
#   dev/e2e/run.sh desk   holds Shift with no drag: the wheel without files (player, layouts)
# Needs kwin_wayland, spectacle, dbus-run-session, python3-gobject (GTK 3), ImageMagick and
# wl-clipboard (and Dolphin for the tap scenario).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SCENARIO=$HERE/scenarios/${1:-full}.txt
OUT=$HERE/out
SOCKET=mcgyver-e2e-$$

if [ "${E2E_INSIDE:-}" != 1 ]; then
    cargo build --release --manifest-path "$ROOT/Cargo.toml"
    cargo build --release --manifest-path "$HERE/drive/Cargo.toml"
    rm -rf "$OUT" && mkdir -p "$OUT"/{config,data,cache,state,files,shots}
    magick -size 64x64 xc:orange "$OUT/files/holiday.jpg"
    python3 -I -c 'import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("photos/beach.txt", "sand\n" * 1000)
    z.writestr("photos/2024/hike.txt", "trail\n" * 1000)' "$OUT/files/photos.zip"
    # Own D-Bus, own config dirs (so nothing touches ~/.config), no tie to the real session's
    # display or lock state.
    env -u WAYLAND_DISPLAY -u DISPLAY -u XDG_SESSION_ID E2E_INSIDE=1 \
        timeout 180 dbus-run-session -- "$0" "$@" 2>/dev/null
    echo "--- app log ($OUT/app.log)"
    grep -vE "key down|keystate" "$OUT/app.log"
    echo "--- screenshots in $OUT/shots"
    ls "$OUT/shots"
    echo "--- files in $OUT/files"
    ls -A "$OUT/files"
    exit 0
fi

export XDG_CONFIG_HOME=$OUT/config XDG_DATA_HOME=$OUT/data XDG_CACHE_HOME=$OUT/cache XDG_STATE_HOME=$OUT/state
# Normally KWin grants every client its restricted protocols. PERMISSIONS=1 keeps its checks, as
# on a real desktop: only programs whose desktop file asks get them (the driver is given one),
# and the app isn't installed, so its settings can do it.
if [ "${PERMISSIONS:-}" = 1 ]; then
    mkdir -p "$OUT/data/applications"
    printf '[Desktop Entry]\nType=Application\nName=e2e driver\nExec="%s"\nNoDisplay=true\nX-KDE-Wayland-Interfaces=org_kde_kwin_fake_input\n' \
        "$HERE/drive/target/release/e2e-drive" > "$OUT/data/applications/e2e-drive.desktop"
    kbuildsycoca6 > /dev/null 2>&1 || true
    CHECKS=
else
    CHECKS=KWIN_WAYLAND_NO_PERMISSION_CHECKS=1
fi
# The welcome plays the first time the app starts; the scenarios start after it (WELCOME=1
# keeps it).
if [ "${WELCOME:-}" != 1 ]; then
    mkdir -p "$OUT/config/mcgyver"
    printf 'welcomed = yes\n' > "$OUT/config/mcgyver/settings.conf"
fi
# The desk scenario switches between keyboard layouts, which KWin serves, speaks English (so
# it has no Language wedge), and turns a stand-in wpctl's volume, which logs to out/wpctl.log.
if [ "${1:-full}" = desk ]; then
    printf '[Layout]\nDisplayNames=,,\nLayoutList=us,tr,de\nUse=true\nVariantList=,,\n' > "$OUT/config/kxkbrc"
    printf '[Translations]\nLANGUAGE=en_US\n' > "$OUT/config/plasma-localerc"
    mkdir -p "$OUT/bin"
    printf '#!/bin/sh\necho "$*" >> "%s/wpctl.log"\n[ "$1" = get-volume ] && echo "Volume: 0.40"\nexit 0\n' \
        "$OUT" > "$OUT/bin/wpctl"
    chmod +x "$OUT/bin/wpctl"
fi
# The dark scenario starts on Breeze's light colours.
if [ "${1:-full}" = dark ]; then
    printf '[General]\nColorScheme=BreezeLight\n\n[Colors:Window]\nBackgroundNormal=239,240,241\n' \
        > "$OUT/config/kdeglobals"
fi
# The quick apps scenario: two stand-in apps, Alpha and Beta, both quick apps in that order.
if [ "${1:-full}" = quickapps ]; then
    mkdir -p "$OUT/data/applications" "$OUT/config/mcgyver"
    for app in Alpha Beta; do
        printf '[Desktop Entry]\nType=Application\nName=%s\nExec=/bin/true\n' "$app" \
            > "$OUT/data/applications/$(echo "$app" | tr '[:upper:]' '[:lower:]').desktop"
    done
    printf 'welcomed = yes\nquick_app = alpha.desktop\nquick_app = beta.desktop\n' \
        > "$OUT/config/mcgyver/settings.conf"
fi
# The share scenario's messaging apps: Telegram and WhatsApp stand-ins that log how they were
# started to out/telegram.log and out/whatsapp.log.
if [ "${1:-full}" = share ]; then
    mkdir -p "$OUT/bin" "$OUT/data/applications"
    for app in Telegram WhatsApp; do
        lower=$(echo "$app" | tr '[:upper:]' '[:lower:]')
        printf '#!/bin/sh\necho "$*" >> "%s/%s.log"\n' "$OUT" "$lower" > "$OUT/bin/$app"
        chmod +x "$OUT/bin/$app"
    done
    printf '[Desktop Entry]\nType=Application\nName=Telegram Desktop\nExec=%s/bin/Telegram -- %%u\n' \
        "$OUT" > "$OUT/data/applications/org.telegram.desktop.desktop"
    printf '[Desktop Entry]\nType=Application\nName=WhatsApp\nExec=%s/bin/WhatsApp %%U\n' \
        "$OUT" > "$OUT/data/applications/whatsapp-for-linux.desktop"
fi
env $CHECKS kwin_wayland --no-lockscreen --virtual \
    --width 1280 --height 800 --socket "$SOCKET" > "$OUT/kwin.log" 2>&1 &
PIDS=($!)
cleanup() {
    kill "${PIDS[@]}" 2>/dev/null || true
    # Services D-Bus started for us (portals, ksecretd, …) outlive the nested session.
    for p in $(pgrep -u "$(id -u)"); do
        [ "$p" != $$ ] && tr '\0' '\n' < "/proc/$p/environ" 2>/dev/null \
            | grep -qxF "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" && kill "$p" 2>/dev/null
    done
    wait 2>/dev/null || true
}
trap cleanup EXIT
for _ in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/$SOCKET" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=$SOCKET OUT ROOT

# --install inside the nested session: its desktop file goes to $OUT/data, its Shift shortcut
# to the nested KWin.
# SETUP runs first, e.g. to leave behind what an older version installed.
[ -n "${SETUP:-}" ] && { sh -c "$SETUP" > "$OUT/setup.log" 2>&1 || true; }
if [ "${PERMISSIONS:-}" != 1 ]; then
    "$ROOT/target/release/mcgyver" --install > "$OUT/install.log" 2>&1 || true
fi
# The desk scenario's player.
if [ "${1:-full}" = desk ]; then
    python3 -I "$HERE/desk_services.py" > "$OUT/desk-services.log" 2>&1 &
    PIDS+=($!)
fi
# MCGYVER_SOUND=0: the clicks would play on the real speakers; MCGYVER_OFFLINE=1: no update
# check reaches the site from a test. PipeWire and PulseAudio are pointed at nothing, so the
# desk can't change the real volume either.
# Stand-ins in out/bin come first.
PATH="$OUT/bin:$PATH" MCGYVER_DEBUG=1 MCGYVER_SOUND=0 MCGYVER_OFFLINE=1 PIPEWIRE_REMOTE=mcgyver-e2e-none PULSE_SERVER=unix:/nonexistent \
    "$ROOT/target/release/mcgyver" > "$OUT/app.log" 2>&1 &
PIDS+=($!)
if [ "${SOURCE:-gtk}" = dolphin ]; then
    QT_QPA_PLATFORM=wayland dolphin --new-window "$OUT/files" > "$OUT/source.log" 2>&1 &
else
    GDK_BACKEND=wayland python3 -I "$HERE/drag_source.py" "file://$OUT/files/${FILE:-holiday.jpg}" > "$OUT/source.log" 2>&1 &
fi
PIDS+=($!)
sleep 3

sed -E "s|^shot (.*)|shell QT_QPA_PLATFORM=wayland timeout 10 spectacle -b -n -f -o $OUT/shots/\1.png|" "$SCENARIO" \
    | "$HERE/drive/target/release/e2e-drive" > "$OUT/drive.log" 2>&1
