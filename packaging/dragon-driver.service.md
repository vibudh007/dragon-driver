# Desktop service unit

The unprivileged daemon receives its Unix listener from the socket unit and restricts its network families to Unix sockets. It starts with the graphical session so desktop wallpaper and PipeWire are available after login. It resumes saved settings even if no GUI opens. The socket can also activate the daemon for an explicit client.
