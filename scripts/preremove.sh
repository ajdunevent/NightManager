#!/usr/bin/bash

systemctl --machine=furios@.host --user disable --now nightynight-off.timer nightynight-on.timer
