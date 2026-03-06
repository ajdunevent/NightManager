# Night Manager


### Purpose:
* To keep your FLX1s from disturbing your sleep


### Done:
* Evening time and morning time are systemd timers for efficiency and flexibility
* At set evening time:
  * Store current states of anything being changed (for restoration in the morning)
  * Set Feedback Profile to Quiet (vibration only) or Silent as configured
  * Optionally set Theme Style to Dark
  * Optionally change Accent Color
  * Optionally change Icon Theme 
  * Optionally enable Ambient Light Sensor (automatic brightness adjustment)
  * Optionally enable Night Light
* At set morning time:
  * If Feedback Profile change was configured, restore original
  * If Theme Style change was configured, restore original
  * If Accent Color change was configured, restore original
  * If Icon Theme change was configured, restore original
  * If Ambient Light Sensor was configured, restore original
  * If Night Light was configured, restore original


### To Do:
* Have option to schedule changes separately and/or follow GNOME automatic Night Light time
* Make a settings app and .desktop file for settings changes instead of editing text files


### How to install:
* Download a .deb from the [Releases page](https://github.com/ajdunevent/NightManager/releases)
* Open a terminal
* Run `dpkg -i <path-to-downloaded-file>`


### How to change the times
* **(Optional)** Read more about systemd drop-ins with `man systemd.unit`
* Edit the file `10-override.conf` in `~/.config/systemd/user/nightynight-on.timer.d` (or `...-off...`) and change, add, or remove `OnCalendar` lines to get the desired behavior.
* Run: ```systemctl --user daemon-reload && systemctl --user restart nightynight-on.timer nightynight-off.timer```


### How to uninstall
Run:
```bash
$ systemctl --user disable --now nightynight-on.timer nightynight-off.timer
$ dpkg -r Night-Manager
```


### Shout out
* [FuriLabs](https://furilabs.com) for making the FLX1s, the first mobile Linux phone I've ever been able to comfortably (enjoyably even!) daily drive.
