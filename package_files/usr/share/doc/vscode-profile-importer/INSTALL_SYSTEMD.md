Systemd user unit example

The contrib/systemd/user directory contains example units for running the
importer as a user service/timer. To install for the current user:

1. Copy the files into your user systemd directory:

   mkdir -p ~/.config/systemd/user
   cp contrib/systemd/user/vscode-profile-importer-sync.* ~/.config/systemd/user/

2. Edit the service file to point ExecStart at the desired profile file. For
   example, change /path/to/profile.code-profile to the actual exported
   profile path.

3. Reload user systemd and enable the timer:

   systemctl --user daemon-reload
   systemctl --user enable --now vscode-profile-importer-sync.timer

The timer will run the service periodically (default: every 6 hours) and use
--non-interactive to avoid prompts. Remove or adjust the schedule as needed.
