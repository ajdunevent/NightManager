#!/usr/bin/bash

sudo -u furios systemctl --machine=furios@.host --user disable --now nightynight-off.timer nightynight-on.timer
