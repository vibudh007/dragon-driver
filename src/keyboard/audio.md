# Audio spectrum for reactive lighting

`Spectrum::default` allocates reusable space for 4,096 samples and computes the Hann window used by the vendor. `Spectrum::analyze` takes one window of signed 16-bit mono PCM plus a sample rate and a frequency interval. It normalizes to the vendor's 32768 scale, executes a real-input transform, excludes the first two frequency bins, and keeps the peak level per band. The amplitude multiplier is 0.15, and the five level thresholds are 0.1, 0.25, 0.4, 0.55, and 0.7. Invalid inputs are rejected before altering the destination.

`fft` applies the in-place radix-2 transform to bit-reversed samples. `Complex::rotate` computes the complex multiplication for each stage. Offline tests check silence and a single tone. A session consumes mono PCM from a caller-provided Linux audio reader and renders the levels to USB lighting packets. The frame period follows the PCM window duration with a 16 ms minimum. The audio source remains a caller choice, so the backend adds no Linux audio service dependency. Vendor transform · Vendor level thresholds

`Bars::push` reassembles split PCM reads into 1,024-sample hops and analyzes a rolling 2,048-sample Hann window. At 44.1 kHz the first window spans 46 ms and subsequent frames arrive every 23 ms. It groups frequency peaks into 22 logarithmic bands from 40 Hz to 16 kHz, converts energy to six levels, and drops quiet bars gradually. Only the per-user daemon analyzes the new PipeWire stream.

The FFT stores one sequence of phase coefficients per stage in each Spectrum. The sequence uses the same recurrence as the original butterflies. Transform calls reuse it across all groups and frames.
