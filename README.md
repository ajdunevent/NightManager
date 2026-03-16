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
  * Optionally change Icon Theme (perfect with [Adwaita-colors](https://github.com/dpejoh/Adwaita-colors)!)
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


### Installation:
```bash
wget -O NightManager_latest.deb https://github.com/ajdunevent/NightManager/releases/latest/download/NightManager_latest.deb
sudo apt install ./NightManager_latest.deb
```


### Configuration:
* Launch the (beta) "NightManager Configuration" from your app launcher.
* Or do things the old way:
  * Edit `~/.config/nightmanager/nightmanager.conf` to set the desired nighttime mode behaviors.
  * Set nighttime enable time(s) using `OnCalendar` line(s) in `~/.config/systemd/user/nightmanager-on.timer.d/10-override.conf`
  * Set nighttime disable time(s) using `OnCalendar` line(s) in `~/.config/systemd/user/nightmanager-off.timer.d/10-override.conf`
  * Run: `systemctl --user daemon-reload && systemctl --user restart nightmanager-on.timer nightmanager-off.timer`


### Uninstallation:
```bash
sudo apt remove nightmanager
```


### Shout out
* [FuriLabs](https://furilabs.com) for making the FLX1s, the first mobile Linux phone many of us have ever been able to comfortably (enjoyably even!) daily drive.
