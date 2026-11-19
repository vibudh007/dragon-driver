# SDK timer replacement

The global exports at `0x10004c40`–`0x10004d90` and the handle-based exports at `0x10008820`–`0x10008940` wrap Windows socket waits and a multimedia timer. The Windows app's HID worker initializes the latter and passes 100 microseconds in its wait loop.

`MicroTimer::start()` allows waits. `wait(microseconds)` waits on a condition variable for the requested interval or cancellation. `stop()` cancels active waits. A generation counter keeps cancelled waits from resuming if a new start races with their wake-up.

This substitutes a Linux worker wait for the Windows multimedia timer and private socket. Linux scheduling can overshoot the requested duration. The timer has no live worker integration yet.
