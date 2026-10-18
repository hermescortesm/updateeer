# dmgbuild settings: the classic installer window (app on the left, Applications on the right).
# Used by tools/package-macos.sh:  dmgbuild -s packaging/macos/dmg-settings.py -D app=... -D icon=... Updateeer out.dmg
import os

app = defines["app"]  # noqa: F821 (provided by dmgbuild)
files = [app]
symlinks = {"Applications": "/Applications"}
icon = defines.get("icon")  # volume icon  # noqa: F821
background = "packaging/macos/dmg-background.png"  # run from the repo root; the @2x next to it is picked up
format = "UDZO"
window_rect = ((200, 120), (660, 400))
default_view = "icon-view"
show_toolbar = False
show_status_bar = False
show_sidebar = False
icon_size = 128
text_size = 13
arrange_by = None
icon_locations = {os.path.basename(app): (170, 190), "Applications": (490, 190)}
