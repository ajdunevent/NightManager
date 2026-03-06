#!/usr/bin/bash

systemctl daemon-reload
sudo -u furios systemctl --user enable --now nightynight-off.timer nightynight-on.timer
