#!/usr/bin/env python3
"""A stand-in for what the desk reads on a Plasma session, on the nested session's bus: a music
player over MPRIS that is playing. (The nested KWin serves the keyboard layouts itself, from
the kxkbrc run.sh writes.) Every call it gets is written to $OUT/desk.log, one per line."""
import os
import sys

from gi.repository import Gio, GLib

LOG = os.path.join(os.environ["OUT"], "desk.log")

MPRIS = """
<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <property name="Identity" type="s" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="PlayPause"/>
    <method name="Next"/>
    <method name="Previous"/>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="Shuffle" type="b" access="readwrite"/>
    <property name="LoopStatus" type="s" access="readwrite"/>
  </interface>
</node>
"""

state = {
    "PlaybackStatus": "Playing",
    "Shuffle": False,
    "LoopStatus": "None",
    "track": 0,
}
TRACKS = [("Teardrop", "Massive Attack"), ("Angel", "Massive Attack")]


def log(line):
    with open(LOG, "a") as f:
        f.write(line + "\n")


def mpris_property(conn, sender, path, interface, name):
    title, artist = TRACKS[state["track"] % len(TRACKS)]
    values = {
        "Identity": GLib.Variant("s", "E2E Player"),
        "PlaybackStatus": GLib.Variant("s", state["PlaybackStatus"]),
        "Metadata": GLib.Variant(
            "a{sv}",
            {
                "xesam:title": GLib.Variant("s", title),
                "xesam:artist": GLib.Variant("as", [artist]),
            },
        ),
        "CanGoNext": GLib.Variant("b", True),
        "CanGoPrevious": GLib.Variant("b", True),
        "Shuffle": GLib.Variant("b", state["Shuffle"]),
        "LoopStatus": GLib.Variant("s", state["LoopStatus"]),
    }
    return values.get(name)


def mpris_set(conn, sender, path, interface, name, value):
    state[name] = value.unpack()
    log(f"set {name} {state[name]}")
    return True


def mpris_call(conn, sender, path, interface, method, params, invocation):
    log(method)
    if method == "PlayPause":
        state["PlaybackStatus"] = "Paused" if state["PlaybackStatus"] == "Playing" else "Playing"
    elif method == "Next":
        state["track"] += 1
    elif method == "Previous":
        state["track"] -= 1
    invocation.return_value(None)


def main():
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    mpris = Gio.DBusNodeInfo.new_for_xml(MPRIS)
    for interface in mpris.interfaces:
        bus.register_object(
            "/org/mpris/MediaPlayer2", interface, mpris_call, mpris_property, mpris_set
        )
    Gio.bus_own_name_on_connection(
        bus, "org.mpris.MediaPlayer2.e2e", Gio.BusNameOwnerFlags.NONE, None, None
    )
    log("ready")
    GLib.MainLoop().run()


if __name__ == "__main__":
    sys.exit(main())
