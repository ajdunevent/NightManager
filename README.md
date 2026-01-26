# Night Manager


### Purpose:
* To keep your FLX1s from disturbing your sleep


### Done:
* Evening time and morning time are systemd timers for efficiency and flexibility
* At set evening time:
  * Set Feedback Profile to Quiet (vibration only) or Silent as configured
  * Optionally set Theme Style to Dark
  * Optionally enable Ambient Light Sensor (automatic brightness adjustment)
  * Optionally enable Night Light
* At set morning time:
  * If Feedback Profile change was configured, set it to Full
  * If Theme Style change was configured, set it to Default
  * If Ambient Light Sensor was configured, disable it
  * If Night Light was configured, disable it


### To Do:
* Add "check" flag to script that selects the correct "on" or "off" state
  * ... but if it can do that, then it should just always check
* Set up a third systemd timer (using OnBootSec delay?) that calls nightynight@check.service to make sure the correct on/off service ran last
  * ... unless I do subpoint above
* Store Feedback Profile, Theme Style, Ambient Light Sensor, and Night Light states before changes so they can be restored
* Make a less manual installation method
* Make a settings app and .desktop file for settings changes instead of editing text files


### How to install:
* Move the contents of dot-local_bin to ~/.local/bin
* Move the contents of dot-config_systemd_user to ~/.config/systemd/user/ and edit OnCalendar lines of .timer files to your liking
* Move the contents of dot-config to ~/.config and edit .conf file to your liking
* Run: ```systemctl --user daemon-reload && systemctl --user enable --now nightynight-on.timer nightynight-off.timer```


### How to uninstall
* Run: ```systemctl --user disable --now nightynight-on.timer nightynight-off.timer```
* Remove the files you added to the directories above


### Shout out
* FuriLabs for making the FLX1s, the first mobile Linux phone I've ever been able to comfortably (enjoyably even!) daily drive.


### License
* For now, this project adopts Alaraajavamma's ["Feel free to do what ever you want with this but no guarantees - this will probably explode your phone xD"](https://gitlab.com/Alaraajavamma/fastflx1/) license.
* but I reserve the sole right to change it at any point for any reason and without any notice.
