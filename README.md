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
* Make a less manual installation method
* Make a settings app and .desktop file for settings changes instead of editing text files


### How to install:
* Move the contents of dot-local_bin to ~/.local/bin
* Move the contents of dot-config_systemd_user to ~/.config/systemd/user/ and edit OnCalendar lines of .timer files to your liking
* Move the contents of dot-config_nightynight to ~/.config/nightynight and edit .conf file to your liking
* Run: ```systemctl --user daemon-reload && systemctl --user enable --now nightynight-on.timer nightynight-off.timer```


### How to uninstall
* Run: ```systemctl --user disable --now nightynight-on.timer nightynight-off.timer```
* Remove the files you added to the directories above and any now empty directories that had to be created


### Shout out
* FuriLabs for making the FLX1s, the first mobile Linux phone I've ever been able to comfortably (enjoyably even!) daily drive.


### License
* For now, this project adopts Alaraajavamma's ["Feel free to do what ever you want with this but no guarantees - this will probably explode your phone xD"](https://gitlab.com/Alaraajavamma/fastflx1/) license.
* but I reserve the sole right to change it at any point for any reason and without any notice.
