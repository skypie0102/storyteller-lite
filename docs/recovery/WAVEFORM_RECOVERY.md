# Allocator waveform recovery — v0.39.0

This note records how the old allocator rendered its audio waveform. It is useful because the supplied Lite mockup also makes waveform preview a central visual element, but Lite does not need a heavyweight audio-editor subsystem to reproduce that experience.

## Old frontend implementation

The v0.39.0 Tauri/React frontend generated waveform data entirely in the webview.

For the current preview audio source it approximately did:

1. `fetch(audioSource)`
2. read the response as an `ArrayBuffer`
3. decode with Web Audio `AudioContext.decodeAudioData(...)`
4. read **channel 0** with `getChannelData(0)`
5. reduce the whole channel to **1,200 amplitude buckets**
6. for each bucket, sample at most roughly 80 positions and retain the maximum absolute amplitude
7. clamp visual amplitude to a small floor (`~0.025`) so silence still has a visible baseline
8. cache the resulting peak array by audio/cache key
9. keep only about **12 waveform envelopes** in the frontend cache

The result was then rendered as simple vertical SVG lines around a center axis.

No FFT, spectrogram, or backend waveform database was involved.

## Timeline overlays were independent

The visual timeline layered other review data over the amplitude envelope:

- current allocation bands;
- silence regions;
- sparse transcript ticks;
- playhead;
- ruler labels;
- draggable allocation boundaries in the full historical editor.

Transcript ticks were downsampled for display rather than rendering every raw cue when many were present.

The old full editor also supported zoom levels roughly `1x / 2x / 4x`, panning and fitting the selected allocation. Those editing controls are historical reference and are not required for the first reduced Lite allocator.

## Lite implication

The supplied Lite mockup can be implemented with a deliberately small waveform contract such as:

```text
WaveformPreview {
    duration,
    peaks: Vec<f32>,   // bounded, e.g. ~1000-1500 values
}
```

The Rust side can compute a peak envelope from the staged/temporary preview audio and hand only the bounded array to Slint.

Recommended properties:

- fixed upper bound on peak count independent of audiobook length;
- mono peak envelope is sufficient for review UI;
- no need to persist waveform data as authoritative state;
- cache it with other disposable preview artifacts;
- keep silence markers and transcript cues as separate structured overlays;
- regenerate after restart if preview cache was discarded.

This fits the recovered durability rule: review **decisions** are durable; waveform/preview media are cheap rebuildable UI cache.

## Scope guard

Do not infer from the old waveform editor that Lite must restore:

- arbitrary split/merge editing;
- draggable general-purpose trim boundaries;
- spectrogram analysis;
- multichannel editing;
- persistent waveform databases.

The useful recovered behavior is simply that an informative waveform can be generated cheaply from a small bounded peak envelope.
