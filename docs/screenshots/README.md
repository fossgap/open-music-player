# Screenshots

Live captures of Open Music Player running normally — a real local library
and the real network services (Radio Browser, Apple Podcasts, YouTube
Music), nothing mocked. Regenerate every image with your own library:

```sh
make screenshots
```

This runs `scripts/capture-screenshots.py` under Xvfb. It uses your library
in `~/.local/share/open-music-player`, plays a track, searches live radio
stations and YouTube Music, and loads podcast discovery, exactly as in
normal use. The only test aids are the virtual display, a silent audio
sink so capture does not play through your speakers, and D-Bus actions in
place of mouse clicks. YouTube Music search needs `scripts/setup-youtube-music.sh`
(or `OPEN_MUSIC_PYTHON` pointing at its venv).

| File | Shows |
| --- | --- |
| [main-library.png](main-library.png) | Real album playing: track list, now playing, synchronized lyrics, live waveform (main README screenshot) |
| [radio.png](radio.png) | Live Radio Browser search with a station playing |
| [podcasts.png](podcasts.png) | Podcast discovery (Apple Podcasts) |
| [streaming.png](streaming.png) | Live YouTube Music search results |
| [visualization-spectrum.png](visualization-spectrum.png) | Audio spectrum visualization in Now playing |
| [visualization-mood.png](visualization-mood.png) | Audio-reactive mood scene tinting the whole player |
| [visualization-album-art.png](visualization-album-art.png) | Skyrock radio playing live with the album art visualization |
| [mini-player.png](mini-player.png) | Mini player |
