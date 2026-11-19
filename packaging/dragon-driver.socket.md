# Desktop service socket unit

The user socket listens under the desktop runtime directory. Its parent is private to the user and the socket mode is 0600. The daemon additionally checks same-user credentials. Stopping the socket removes its runtime pathname.
