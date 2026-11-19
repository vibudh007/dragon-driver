# System access socket

The template exposes one mode-0600 Unix socket per authorized UID. The system manager creates it and starts the broker on demand. Enabling the instance under `sockets.target` makes it available after reboot. The server verifies the same UID independently using kernel peer credentials.
