---
name: Open Audio Player
description: A quiet listening room for a native Linux audio player.
colors:
  sage: "#c2dfa3"
  sage-hover: "#d1ebba"
  sage-ink: "#182414"
  window: "#1b201e"
  rail: "#141917"
  raised: "#29312d"
  raised-hover: "#354239"
  raised-active: "#435343"
  field: "#252d28"
  detail: "#202923"
  ivory: "#f1f3eb"
  control-ink: "#e7ede4"
  muted: "#adb9b0"
  outline: "#3c4941"
  divider: "#344037"
  selected: "#34412f"
  nav-selected: "#303c2b"
  nav-selected-ink: "#d1e7b8"
  row-hover: "#29332c"
  notice: "#303c30"
  error-surface: "#442c28"
  error-ink: "#ffb4a9"
typography:
  body:
    fontFamily: '"Adwaita Sans", "Cantarell", sans-serif'
    fontSize: "14px"
  headline:
    fontSize: "32px"
    fontWeight: 750
    letterSpacing: "-0.7px"
  title:
    fontSize: "17px"
    fontWeight: 700
  track:
    fontSize: "14px"
    fontWeight: 650
  label:
    fontSize: "12px"
  time:
    fontSize: "11px"
    fontFeature: '"tnum"'
  lyrics:
    fontSize: "15px"
    lineHeight: 1.6
rounded:
  thumbnail: "7px"
  control: "8px"
  popover: "10px"
  artwork: "12px"
  circle: "999px"
spacing:
  micro: "4px"
  tight: "6px"
  small: "8px"
  inline: "10px"
  related: "12px"
  row: "14px"
  group: "16px"
  section: "20px"
  panel: "24px"
  page: "32px"
components:
  button:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.control-ink}"
    rounded: "{rounded.control}"
    padding: "9px 14px"
  button-hover:
    backgroundColor: "{colors.raised-hover}"
  button-active:
    backgroundColor: "{colors.raised-active}"
  button-primary:
    backgroundColor: "{colors.sage}"
    textColor: "{colors.sage-ink}"
    rounded: "{rounded.control}"
    padding: "11px 18px"
  button-primary-hover:
    backgroundColor: "{colors.sage-hover}"
  button-flat:
    backgroundColor: "transparent"
    textColor: "{colors.control-ink}"
    rounded: "{rounded.control}"
    padding: "8px"
  button-play:
    backgroundColor: "{colors.sage}"
    textColor: "{colors.sage-ink}"
    rounded: "{rounded.circle}"
    padding: "8px"
  toggle-active:
    backgroundColor: "{colors.selected}"
    textColor: "{colors.sage}"
    rounded: "{rounded.control}"
  navigation:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    rounded: "{rounded.control}"
    padding: "13px 12px"
  navigation-selected:
    backgroundColor: "{colors.nav-selected}"
    textColor: "{colors.nav-selected-ink}"
  field:
    backgroundColor: "{colors.field}"
    textColor: "{colors.ivory}"
    rounded: "{rounded.control}"
    padding: "7px 12px"
  track-row:
    backgroundColor: "transparent"
    rounded: "{rounded.control}"
    padding: "9px 8px"
  track-row-hover:
    backgroundColor: "{colors.row-hover}"
  track-row-selected:
    backgroundColor: "{colors.selected}"
  album-art:
    rounded: "{rounded.artwork}"
  player-bar:
    backgroundColor: "{colors.rail}"
    padding: "16px 24px 12px"
  detail-panel:
    backgroundColor: "{colors.detail}"
    padding: "20px"
  notice:
    backgroundColor: "{colors.notice}"
    rounded: "{rounded.control}"
    padding: "10px 12px"
  notice-error:
    backgroundColor: "{colors.error-surface}"
    textColor: "{colors.error-ink}"
---

# Design System: Open Audio Player

## Overview

**Creative North Star: "A quiet listening room"**

Deep green-black surfaces, warm ivory text, and soft sage actions create a calm place to listen. Artwork supplies the individual character of each recording; the surrounding interface stays restrained and legible in low ambient light.

This is a native Linux GTK application in Operate mode. Familiar symbolic icons, system sans typography, a stable navigation rail, and a persistent transport support repeated listening tasks. The library is compact enough to scan while section headings and artwork retain breathing room.

**Key Characteristics:**

- Dark tonal surfaces with one sage interaction accent.
- Native GTK controls and concise action labels.
- A stable library structure and persistent playback controls.
- Real artwork, a softly blended cover backdrop, and a waveform derived from decoded audio.

The implemented sources are `resources/style.css`, the GtkBuilder files in `resources/`, and `src/app.rs`. Reviewed renders are in `.impeccable/review/`. Frontmatter records the reusable values; platform defaults continue to govern properties the application does not override.

## Colors

The palette combines warm text with green-tinted dark surfaces; the artwork can bring additional color without changing the controls.

### Primary

- **Sage** marks primary actions, play/pause, checked controls, the playing track title, seek progress, and keyboard focus. **Sage ink** keeps text and symbols dark on filled sage actions.
- **Sage hover** lightens suggested actions. The circular play control has its own slightly lighter hover treatment in the stylesheet.

### Neutral

- **Window** is the library canvas. **Rail** anchors navigation, the header, and the player; **detail** separates the now-playing panel.
- **Raised** supports ordinary buttons and popovers, with **raised hover** and **raised active** providing restrained feedback. **Field** distinguishes editable controls.
- **Ivory** carries headings and body text; **control ink** carries ordinary controls; **muted** carries metadata, secondary navigation, and fully opaque placeholders.
- **Outline** defines input borders and slider troughs. **Divider** separates the track header, transport, and detail panel.
- **Selected** backs checked controls and selected rows. **Navigation selected** and its lighter text establish the current destination. **Row hover** gives track and album browsing a quiet pointer response.
- **Notice** supports status messages. **Error surface** and **error ink** are reserved for recoverable errors.

**The Sage State Rule.** Use sage to identify an action, focus, or playback state; let album artwork carry decorative color.

## Typography

**Body Font:** Adwaita Sans, Cantarell, then the platform sans-serif. All roles inherit this native family. There is no separate display or monospace family.

The hierarchy relies on size and weight rather than decorative lettering. Page headings are bold and slightly tightened; track titles remain easy to scan above quieter artist metadata. Use the frontmatter roles directly:

- **Headline:** page titles.
- **Title:** section headings and the current track in the detail panel.
- **Track:** track, album, and transport titles.
- **Body:** general interface copy and controls.
- **Label:** metadata, table headings, and compact secondary copy.
- **Time:** elapsed time and duration, with tabular figures.
- **Lyrics:** selectable, wrapped text with generous line spacing.

Long titles and metadata ellipsize in rows and the player. The detail title and lyrics wrap. Keep action labels short and preserve room for translated strings; do not force prose into fixed-width metadata columns.

## Layout

The main window opens at (1240 × 800 px). A header spans the window, a rail sits beside the expanding library, and the transport spans the entire bottom edge. A right-side now-playing panel can join the body without displacing the transport. The rail requests (200 px) of width and scrolls vertically when its full contents do not fit.

The library has page gutters (32 px), a top margin (26 px), and section gaps (20 px). Search expands beside the view switch and page-level controls. Track rows use a two-line title/artist block, album metadata, duration, favorite, and an options menu. The virtualized list scrolls inside the content area; the status footer remains below it. Album view uses a native adaptive grid of two to five columns with artwork requested at (160 × 160 px).

Compact desktop behavior is supported by GTK allocation, ellipsized labels, and independent scroll areas. The reviewed compact window is (824 × 620 px): navigation and the complete transport remain available while fewer rows fit. The main window requests a minimum of (760 × 580 px), but child requisitions can make the effective minimum wider. Do not describe this as a mobile layout or an automatic collapsed rail.

The header toggle explicitly shows or hides the rail. Toggling the detail panel below a window width of (1100 px) hides the rail; this is an interaction rule, not a general resize breakpoint. The detail panel requests (270 px) before its padding and content requisitions, and lyrics have their own scroll area.

The separate, non-resizable mini player defaults to (420 px) wide. It retains artwork, ellipsized title and artist, play/pause, next, and return-to-full-player controls in a single horizontal row.

**The Persistent Transport Rule.** Keep current-track context and playback controls outside scrolling library content.

## Elevation & Depth

Depth comes from tonal surfaces and fine separators. Custom buttons, fields, the header, and slider thumbs explicitly remove shadows. Native window decoration remains platform-owned; do not add floating card shadows to library content.

The signature artwork backdrop uses actual cover pixels: a small, resampled copy is enlarged with bilinear filtering and blended over an artwork-derived dark gradient. The crisp cover remains contained above it. With no artwork, a subdued green gradient is the fallback. This treatment belongs to the now-playing artwork area, not every surface.

## Shapes

Controls and list rows share gently rounded corners. Small track thumbnails use the thumbnail radius; large artwork and album placeholders use the artwork radius; popovers have their own intermediate radius. The main play/pause button is circular. Structural panels remain rectangular so the library reads as one workspace.

Use thin strokes for fields and separators, filled tonal surfaces for selection, and native symbolic icons for actions. The favorite control uses an outlined star when inactive and a filled star when active. Avoid using color alone to communicate this state.

## Components

### Buttons and toggles

Ordinary buttons are quiet raised controls. Suggested actions use a sage fill, dark ink, and a heavier label; circular play/pause is the transport's visual anchor. Flat buttons keep secondary icon actions light. The checked state uses a selected surface and sage foreground.

Button backgrounds transition over (150 ms) with ease-out. Keyboard focus uses a sage outline (2 px) with an offset (2 px); fields receive the same treatment on focus within. Disabled buttons reduce opacity to (0.38). Preserve native keyboard activation and tooltips for symbolic controls.

The track and player favorites are real GTK toggle buttons. Their checked state, filled/outline star, tooltip, and accessible label update together. The label is “Add to favorites” when inactive and “Remove from favorites” when active. The player favorite is disabled when there is no current track.

### Fields

Search and URL inputs use the field surface, a thin outline, and opaque muted placeholders. Search expands to use the available toolbar width. Native selection, keyboard editing, and focus behavior remain intact; Ctrl+F focuses search.

### Navigation

The stable rail groups the listening destinations with symbolic icons and left-aligned labels. Default navigation is muted and transparent; hover raises contrast; a selected tonal fill identifies the current page. Queue and settings sit lower in the rail and remain reachable through vertical scrolling at short heights.

### Track rows and album tiles

Track rows are flat at rest, quietly highlighted on hover, and tonal when selected. The playing title turns sage independently of selection. Artwork requests (44 × 44 px) in a row, with a native audio symbol on a muted tile when missing. Favorite and options remain separate controls. Track activation uses the list's native double-click behavior.

Album tiles pair square artwork with a bold title and muted artist. They have no enclosing card border or shadow; the grid child supplies hover feedback. Album activation uses a single click. Do not turn each metadata group into an additional card.

### Persistent player and mini player

The main player combines cover art, current-track metadata, favorite, shuffle/previous/play/next/repeat, time and seek, volume, and the mini-player action. Cover activation opens now-playing and lyrics. Seek becomes interactive only when loaded audio has a known positive duration. The mini player preserves the same title, artwork, and play state with a reduced control set.

### Now playing, artwork, and waveform

The detail panel uses its own surface, a left divider, and an artwork backdrop above title, artist, waveform, and lyrics. The cover is contained inside the backdrop rather than cropped. Lyrics wrap and scroll independently.

The live waveform is a thin sage trace of signed, decoded audio samples in a (64 px) tall drawing area. It is a time-domain waveform, not a frequency spectrum or a decorative equalizer. The UI samples updates on a (100 ms) timer; when playback stops, the trace decays toward its centerline. The internal legacy `spectrum` identifier does not change the feature's meaning. Content changes use a short native crossfade; detail reveal uses GTK's slide transition.

**The Real Media Rule.** Artwork and waveform behavior must be driven by the current media; missing artwork gets a deliberate placeholder and silence does not produce invented activity.

### Notices and dialogs

Status and error notices are inline, wrapped, and dismissible, with a native spinner for ongoing work. Settings and playlist editing use native GTK windows, fields, switches, dropdowns, and conventional trailing actions. Maintain their shared spacing, type, control shapes, and palette rather than introducing another visual system.

## Do's and Don'ts

### Do:

- **Do** keep the rail, library, detail panel, and transport legible as connected tonal regions.
- **Do** use sage for primary actions, focus, checked controls, and current playback state.
- **Do** preserve visible keyboard focus and native accessible control semantics.
- **Do** update favorite icon, checked state, tooltip, and accessible label together.
- **Do** use real media artwork and decoded audio for the backdrop and waveform.
- **Do** keep navigation and library content scrollable while playback controls stay available.
- **Do** retain ellipsis, wrapping, and concise localized action labels where the existing components use them.

### Don't:

- **Don't** replace the native system sans or symbolic icon language with unrelated decorative styles.
- **Don't** add card shadows or extra container borders to the flat library rows and album tiles.
- **Don't** use a frequency spectrum, random bars, or looping decoration in place of the audio waveform.
- **Don't** hide a favorite's state behind color alone or reduce placeholder contrast through opacity.
- **Don't** claim automatic narrow-window navigation collapse; the rail has an explicit toggle and a detail-panel interaction rule.
