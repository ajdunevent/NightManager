#!/usr/bin/bash

systemctl daemon-reload
systemctl --machine=furios@.host --user enable --now nightynight-off.timer nightynight-on.timer
