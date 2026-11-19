# AppImage entry point

`AppRun` checks whether the host libraries can load the frontend and meet its GTK 4.8 minimum before choosing the runtime. Using the compatible host GTK lets modern desktop themes use the same CSS syntax as the rest of the desktop. The bundled stack handles hosts without compatible libraries. `--cli` selects the CLI and uses its version check to test library loading. `--setup` and `--uninstall-service` run the host companion outside the bundled library environment.
