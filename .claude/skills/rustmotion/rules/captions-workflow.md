# Rule: Captions Workflow (audio → word timings → caption component)

Word-synced captions are a two-step pipeline: generate word timings with
`rustmotion captions`, then inject them into a scenario through the variables
system. Never hand-write word timings for real voiceovers.

## Step 1 — Generate word timings

```bash
# From audio (requires whisper.cpp: brew install whisper-cpp)
rustmotion captions voice.mp3 -o words.json

# Pick a model / force the language
rustmotion captions voice.mp3 -o words.json --model small --lang fr

# Offline import from existing subtitles (no whisper needed)
rustmotion captions --from-srt subs.srt -o words.json
rustmotion captions --from-vtt subs.vtt -o words.json
```

Output shape (also valid as a `--props` file, on purpose):

```json
{
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.32 },
    { "text": "world", "start": 0.32, "end": 0.7 }
  ]
}
```

- Transcription uses a whisper.cpp binary found in PATH (`whisper-cli`,
  `whisper-cpp`, `main`). Models are resolved from
  `~/.cache/whisper/ggml-<name>.bin` or a direct `.bin` path.
- **SRT/VTT approximation**: subtitle cues carry sentence-level timing only,
  so each cue's duration is spread **uniformly** across its words. Good
  enough for karaoke modes; for precise word pops prefer real transcription.

## Step 2 — Inject into the scenario

Declare a `words` variable in `config` and reference it from the caption
component:

```json
{
  "config": { "words": { "type": "array", "default": [] } },
  "scenes": [
    {
      "duration": 5,
      "children": [
        {
          "type": "caption",
          "mode": "word_pop",
          "words": "$words",
          "active_color": "#FFFFFF",
          "pill_color": "#FF3366",
          "position": { "x": 0, "y": 1600 },
          "style": { "width": "100%", "font-size": 72 }
        }
      ]
    }
  ]
}
```

```bash
rustmotion render -f scenario.json --props words.json
```

## Caption modes

| Mode | Behavior |
|---|---|
| `highlight` (default) | All words visible, active word takes `active_color` |
| `karaoke` | Same as highlight (karaoke-style progression) |
| `word_by_word` | Only the active word, centered, no pill |
| `word_pop` | TikTok style: only the active word, spring scale-in, pill background |
| `karaoke_pop` | Full line visible + active word scales 1.15x with `active_color` and pill |

- `pill_color` (word_pop / karaoke_pop): pill background behind the active
  word. Defaults to black at 70% (`#000000B3`).
- `max_width`: wraps the line in highlight/karaoke/karaoke_pop modes — always
  set it on vertical formats to avoid viewport overflow.
- The caption paints from its baseline: position it with enough headroom
  (font-size + pill padding above the anchor point).
