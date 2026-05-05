#!/bin/sh
update-desktop-database /usr/share/applications || true
gtk-update-icon-cache /usr/share/pixmaps || true
