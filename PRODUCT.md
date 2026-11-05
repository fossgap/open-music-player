# Open Audio Player

<!-- impeccable:product-schema 1 -->

## Platform

Linux desktop (native GTK).

## Stack

Rust, GTK, and maintainable GtkBuilder UI files, as required by the original README and confirmed by the user.

## Product Purpose

A fast, lightweight home for local music, podcasts, internet radio, and streaming audio, with playback that continues in the background.

## Users

Linux listeners. More specific audience research has not been supplied.

## Capabilities and Constraints

- Selected-folder music discovery, library management, favorites, artwork, lyrics, and audio visualization.
- Podcast subscriptions, radio discovery, and streaming-service integration.
- Session persistence, MPRIS desktop controls, a system tray, and a mini player.
- English, French, Spanish, German, Italian, Portuguese, Russian, Chinese, Japanese, and Korean.
- Docker and a development container; Debian, RPM, Arch, and AppImage packaging.
- Concise UI copy and responsive interaction.

## Evidence on Hand

The initial README is the product brief. There is no existing implementation, brand artwork, licensed music, service account, or streaming credential. Rhythmbox and Clementine are references from the brief.

## Product Principles

- Keep the library and playback controls immediately accessible.
- Perform discovery and network requests away from the UI thread.
- Keep personal listening data on the device.
- Distinguish playable media from services that require an external account or client.

## Open Decisions

No distribution license or service credentials have been provided. Visual details and implementation choices are delegated by the request to design and implement the application.
