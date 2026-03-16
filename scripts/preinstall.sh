#!/usr/bin/bash

case "$1" in
  install|upgrade)

    # Cleanup timers on upgrade
    if [ "$1" = "upgrade" ]; then
      systemctl --machine=furios@.host --user stop nightynight-on.timer nightynight-off.timer >/dev/null 2>&1
      systemctl --machine=furios@.host --user disable nightynight-on.timer nightynight-off.timer >/dev/null 2>&1
    fi
    
    # Migrate systemd overrides if found
    if [ -d "/home/furios/.config/systemd/user/nightynight-off.timer.d/" ] && [ ! -d "/home/furios/.config/systemd/user/nightmanager-off.timer.d/" ]; then
      mv "/home/furios/.config/systemd/user/nightynight-off.timer.d/" "/home/furios/.config/systemd/user/nightmanager-off.timer.d/"
    fi
    if [ -d "/home/furios/.config/systemd/user/nightynight-on.timer.d/" ] && [ ! -d "/home/furios/.config/systemd/user/nightmanager-on.timer.d/" ]; then
      mv "/home/furios/.config/systemd/user/nightynight-on.timer.d/" "/home/furios/.config/systemd/user/nightmanager-on.timer.d/"
    fi

    # Migrate config if found
    if [ -d "/home/furios/.config/nightynight/" ]; then
      # Rename the internal file first to match the new naming scheme
      if [ -f "/home/furios/.config/nightynight/nightynight.conf" ]; then
        mv "/home/furios/.config/nightynight/nightynight.conf" "/home/furios/.config/nightynight/nightmanager.conf"
      fi

      if [ ! -d "/home/furios/.config/nightmanager/" ]; then
        # New folder doesn't exist, rename the whole directory
        mv "/home/furios/.config/nightynight/" "/home/furios/.config/nightmanager/"
      else
        # New folder exists, move the file into it
        if [ -f "/home/furios/.config/nightynight/nightmanager.conf" ]; then
          if [ ! -f "/home/furios/.config/nightmanager/nightmanager.conf" ]; then
            mv "/home/furios/.config/nightynight/nightmanager.conf" "/home/furios/.config/nightmanager/nightmanager.conf"
          else
            # Collision: keep old as a backup
            mv "/home/furios/.config/nightynight/nightmanager.conf" "/home/furios/.config/nightmanager/nightmanager.conf.saved"
          fi
        fi
        # Clean up old directory
        rm -rf "/home/furios/.config/nightynight/"
      fi
    fi

    # Migrate state directory if found
    if [ -d "/home/furios/.local/state/nightynight/" ] && [ ! -d "/home/furios/.local/state/nightmanager/" ]; then
      mv "/home/furios/.local/state/nightynight/" "/home/furios/.local/state/nightmanager/"
    fi

  ;;

  abort-upgrade)
  ;;

  *)
    exit 0
  ;;
esac

exit 0
