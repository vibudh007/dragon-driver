# Responsive picker columns

`monitor_columns` reads the window and monitor widths in logical pixels. It updates on the GDK surface's layout and monitor-enter signals and when fullscreen state changes. `columns` chooses one column below a third of the monitor, two from one third, and three from two thirds or in fullscreen.

`flow_columns` updates the existing GtkFlowBox in place. It preserves item instances, order, focus, and selection. The calibration dropdown uses one scroller for all columns and accessible left and right modifier names for glyphs that look alike.
