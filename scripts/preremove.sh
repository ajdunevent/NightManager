#!/usr/bin/bash

systemctl --machine=furios@.host --user disable --now nightmanager-off.timer nightmanager-on.timer
