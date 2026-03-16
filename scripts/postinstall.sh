#!/usr/bin/bash

chown -R furios:furios /home/furios/.config/nightmanager/
systemctl --machine=furios@.host --user daemon-reload
systemctl --machine=furios@.host --user enable --now nightmanager-off.timer nightmanager-on.timer
