#!/usr/bin/bash

systemctl daemon-reload
sudo -u furios systemctl --machine=furios@.host --user enable --now nightynight-off.timer nightynight-on.timer
