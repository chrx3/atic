# Version history

Español: [CHANGELOG.md](CHANGELOG.md)

What people using Atic notice in each version, newest first. It is the same text the app shows in "What's new" after an update (`apps/atic/assets/changelog.json`); this file is generated from it with `node scripts/gen-changelog.mjs`. Downloads are on [Releases](https://github.com/chrx3/atic/releases).


## 0.5.6 — 2026-11-01

- **New:** Captures has a bar to switch between Capture and Record, with microphone, sound and pill one click away
- **New:** On Mac, Edit your recordings: autofocus on your clicks and where you type, 18 image backgrounds (or your own), text and cuts
- **New:** On Mac, export your recordings as vertical, square or 4:5, as GIF or HEVC, with automatic captions generated on your computer, no internet
- **New:** On Mac, the editor gains an Assistant: ask Claude, Codex, OpenCode or Cursor for changes, and pick the model
- **New:** Live captions in Meetings with the local model: no internet or key needed, and the audio never leaves your computer
- **Improved:** On Mac, recordings take about half the space (HEVC), pausing no longer cuts the video and there's a smooth cursor
- **Improved:** The recording editor now looks exactly like the export and its camera follows the mouse without hugging the edge; tune the cursor, click and motion blur, plus undo, speed and blurred areas
- **Improved:** Copy text now separates columns: in an interface with a sidebar and panels, the selection no longer spills into the next column
- **Fixed:** On notifications you can't reply to, Dismiss now sits at the end of the row and the text shows more

## 0.5.5 — 2026-10-31

- **New:** System → Performance: live CPU, GPU, memory and network, with temperatures, disks and battery
- **New:** Free up space: finds builds and caches you can delete and moves them to the Trash
- **New:** System → Displays: resolution, refresh rate, main display, mirroring and dark mode
- **New:** What's New: after an update, Atic tells you what changed
- **New:** The PC can control the phone while viewing its screen
- **New:** Copy text respects columns and paragraphs, and copies tables as tables
- **Fixed:** Meetings no longer loses work when deleting, retranscribing or summarizing, and recovers cut recordings
- **Fixed:** The X to delete Flip pages no longer disappears under the mouse
- **Fixed:** Capture, Flip, Board and the eyedropper no longer get stuck or lock up your computer

## 0.5.4 — 2026-10-29

- **New:** Text and buzzes between two linked Atic installs
- **New:** Shared-screen video with network-adaptive bitrate
- **Improved:** The phone's screen is hardware-decoded on macOS
- **Improved:** Sharing the PC screen uses hardware encoding and recovers on its own on a bad network
- **Fixed:** The Atic Code preview works on Windows

## 0.5.3 — 2026-10-28

- **New:** See your phone's screen on the PC and the PC's on your phone, with absolute touch
- **New:** Web preview in Atic Code
- **New:** Share links and photos between phone and PC
- **Improved:** In Atic Code text selection flows from one message to the next
- **Fixed:** Atic Code lets you select and copy text, and its consoles scroll
- **Fixed:** Lyrics with a translation show only the original in the verse

## 0.5.2 — 2026-10-26

- **New:** Atic Code guides you through installing Node and Claude Code
- **Fixed:** The wheel no longer closes the tool it opens

## 0.5.1 — 2026-10-26

- **New:** Pomodoro in the pill
- **New:** Chat with Atic Code in the Agents panel
- **New:** Tasks in Texts, also from Meetings summaries
- **New:** Side-by-side panels and a glances carousel
- **Fixed:** The magnifier no longer lags and the crosshair and eyedropper work on every screen
- **Fixed:** Clicks reach other apps again after a capture

## 0.5.0 — 2026-10-24

- **New:** New step-by-step welcome with permissions and a tour you can replay from Settings
- **New:** Phone and PC find each other on the same network, with files, notifications, calls and touch control
- **New:** Dictation and Meetings with the local Parakeet v3 voice engine and custom prompts
- **New:** The whole app in English as well as Spanish
- **Improved:** Fluid transitions, lyrics carousel and selections that slide like liquid
- **Improved:** Settings and Meetings in Material 3 Expressive and Liquid Glass on macOS 26

## 0.4.50 — 2026-10-19

- **New:** The Now Playing glance morphs into the panel
- **Improved:** Windows is smoother and uses less memory
- **Improved:** The capture tray uses less memory and lets the pill sleep
- **Fixed:** The Clipboard no longer rereads images or keeps them in memory

## 0.4.49 — 2026-10-17

- **New:** Atic for macOS now runs on the new pill, with physical notch and macOS voice
- **New:** Meetings lets you pick the microphone and falls back if it is silent
- **New:** Now Playing unfolds from the cover art with scroll
- **New:** The Board groups its tools in islands, like Flip's dock
- **New:** Atic Code closes idle conversations after a while
- **Fixed:** On macOS dragging from the Clipboard no longer freezes the pill

## 0.4.48 — 2026-10-15

- **New:** Atic Code adds ACP agents
- **Improved:** Setting up Meetings is easier

## 0.4.47 — 2026-10-13

- **New:** The Board adds text, arrows, pixelate, magnifier, stamps and export
- **New:** Send what you draw on the Board to Flip or to an agent
- **New:** Flip imports meetings with their summary and tasks as checkboxes
- **New:** Quick settings when you hover over the notch face
- **Improved:** Atic Code ships its agent in the installer and polishes Liquid Glass
- **Fixed:** AltGr no longer triggers Alt shortcuts

## 0.4.46 — 2026-10-10

- **New:** Atic Code: your agents workspace in the pill, with folders, panes and changes
- **New:** Atic now runs as the native pill, with its own Settings, tray and welcome
- **New:** Link the phone app: music, agent questions and permissions from your phone
- **New:** Meetings, Clipboard and side notch rebuilt
- **New:** Optional local Whisper for dictation
- **New:** Light theme for Atic Code, Settings and Meetings

## 0.4.44 — 2026-10-02

- **New:** The island opens as a single object that stretches
- **New:** Hide the lyrics and agent text when idle
- **New:** Flip supports drawing on top, text without background and checklists with jumps
- **Fixed:** The floating wheel no longer opens and closes by itself
- **Fixed:** The notch docks on top across monitors of different heights
- **Fixed:** Subagents no longer overlap on the Board

## 0.4.43 — 2026-09-28

- **Fixed:** On macOS the pill works with Spaces per display
- **Fixed:** On macOS dropping onto the Board lands under the cursor and accepts history text
- **Fixed:** On macOS dictation goes to the Atic console, not the external app

## 0.4.42 — 2026-09-27

- **New:** Administrator console and a more complete Flip
- **Fixed:** Window fixes

## 0.4.41 — 2026-09-26

- **New:** Board with files and spaces that save themselves
- **New:** The launcher converts currencies in its calculator
- **New:** Consoles with WebGL and zoom-aware selection
- **Improved:** The pill uses less power with lighter animations and fewer system reads
- **Fixed:** Dictation returns the text to the console where you started
- **Fixed:** Chat replies are better formatted and long threads are lighter

## 0.4.40 — 2026-09-25

- **Fixed:** On macOS OCR no longer waits minutes for Vision to compile

## 0.4.39 — 2026-09-24

- **New:** macOS support
- **New:** Console board with resumable sessions and live activity
- **New:** Glances, pill customization, a chosen home and re-docking
- **Improved:** Fixes for volume, clicks and icons on Windows
- **Fixed:** Audio shows one row per app and volume applies to all its sessions
- **Fixed:** Clipboard dragging works with web terminals and large images

## 0.4.38 — 2026-09-21

- **Fixed:** The history no longer overflows its box and agent logos fit

## 0.4.37 — 2026-09-21

- **New:** Per-agent quotas and dragging from history
- **New:** Real system resources, threshold alerts, coffee mode and volume with the wheel
- **New:** Dedicated consoles window
- **Improved:** The window's five tools are redesigned with a unified look
- **Improved:** Better contrast for secondary text in all palettes
- **Fixed:** More robust MCP servers

## 0.4.36 — 2026-09-13

- **New:** Permissions screen on macOS
- **Fixed:** Fixed audio on macOS and a capture that would hang

## 0.4.35 — 2026-09-13

- **New:** Atic arrives on macOS: capture, Board, shelf, OCR and system audio
- **New:** macOS launcher with system apps, icons and Recents
- **New:** The pill can cross over to the neighboring monitor
- **Improved:** Shortcuts are captured by physical key and warn about duplicates
- **Fixed:** The shelf no longer activates Atic when dragged

## 0.4.34 — 2026-09-12

- **New:** The launcher grows in place, does calculations and closes apps

## 0.4.33 — 2026-09-12

- **Improved:** Atic uses fewer resources at rest by releasing the preloaded capture

## 0.4.32 — 2026-09-11

- **Improved:** Internal and stability improvements

## 0.4.31 — 2026-09-11

- **Fixed:** The console's confirm button can be clicked in the overlay

## 0.4.30 — 2026-09-10

- **Fixed:** The update chip receives clicks while the pill is docked
- **Fixed:** Capture no longer waits for a preview that never arrives

## 0.4.29 — 2026-09-10

- **New:** Orchestrate agents with each other over MCP and connect them from the interface
- **New:** Agents usage panel
- **New:** The window's back side is a notes board you can export with photos and text
- **New:** Register global shortcuts from Settings
- **Improved:** Redesigned capture tray and clipboard preview

## 0.4.28 — 2026-09-05

- **New:** Eyedropper to pick colors from the screen
- **New:** The clipboard history shows copied colors

## 0.4.27 — 2026-09-03

- **Improved:** Improvements to the pill, the overlay and tool anchoring

## 0.4.26 — 2026-09-01

- **New:** Choose the overlay size in Settings
- **Fixed:** Fixed the overlay zoom

## 0.4.25 — 2026-09-01

- **Improved:** General polish of agents, pill and overlay

## 0.4.24 — 2026-08-30

- **New:** The pill shows the agents' usage quotas
- **Improved:** Improved agents chip and pill home

## 0.4.23 — 2026-08-28

- **Improved:** Main window, pill and themes redesigned

## 0.4.22 — 2026-08-27

- **New:** Clipboard preview, text dragging and a movable wheel

## 0.4.21 — 2026-08-26

- **New:** Embedded consoles with as many tabs as you want, instead of two fixed ones
- **New:** Compact agents launcher that grows from the pill, with each brand's icon
- **New:** Pick the starting folder for each agent console
- **Improved:** Better splits, panels and shortcuts in the agent consoles

## 0.4.20 — 2026-08-21

- **Improved:** The interface now uses Chilean Spanish

## 0.4.19 — 2026-08-20

- **Fixed:** Checking for updates no longer freezes the main window

## 0.4.18 — 2026-08-20

- **Fixed:** The tools wheel moves with the mouse wheel again

## 0.4.17 — 2026-08-20

- **Fixed:** The main window responds to clicks again after moving the pill

## 0.4.16 — 2026-08-20

- **Fixed:** The mouse wheel works in the main window outside the pill

## 0.4.15 — 2026-08-20

- **Fixed:** Attaching a file no longer interrupts the picker's spin

## 0.4.14 — 2026-08-20

- **Fixed:** The overlay no longer covers the window on screens with mixed DPI

## 0.4.13 — 2026-08-20

- **New:** Atic follows the system language
- **Fixed:** The overlay repositions correctly when the computer wakes up

## 0.4.12 — 2026-08-20

- **Fixed:** You can download the update even while the app is still checking

## 0.4.11 — 2026-08-20

- **New:** Choose the interface language
- **Improved:** Summaries and transcriptions show clearer errors

## 0.4.10 — 2026-08-19

- **Improved:** The update notice and the About section show download progress more clearly

## 0.4.9 — 2026-08-19

- **Improved:** Internal and stability improvements

## 0.4.8 — 2026-08-19

- **Improved:** Internal and stability improvements

## 0.4.7 — 2026-08-19

- **Fixed:** The tutorial closes if it was opened by accident
- **Fixed:** The second monitor is covered when starting with Windows

## 0.4.6 — 2026-08-19

- **New:** Meetings can transcribe with Groq and adds an Info tab
- **New:** Automatic app updates

## 0.4.5 — 2026-08-18

- **New:** Draw over the frozen screen or a freshly taken capture
- **New:** The pill shows the status of agents running in its terminal
- **Improved:** The launcher now appears as a circle

## 0.4.4 — 2026-08-17

- **Fixed:** The capture shortcut no longer closes the clipboard or the pill
- **Fixed:** Floating tools are no longer clipped at the edges

## 0.4.3 — 2026-08-17

- **Fixed:** Capture selection no longer drifts on multiple screens with different DPI

## 0.4.2 — 2026-08-17

- **Fixed:** The capture overlay covers the whole desktop instead of being cropped to one screen

## 0.4.1 — 2026-08-16

- **Improved:** The pill can reach the top edge of the screen
- **Improved:** The overlay follows the light theme
- **Fixed:** Summoning from the edge restores the floating size

## 0.4.0 — 2026-08-16

- **New:** The pill docks to screen edges as a tools island
- **Improved:** The liquid effect is more viscous and the wheel no longer bounces
- **Fixed:** Fixes for capture and clipboard

## 0.3.2 — 2026-08-09

- **New:** Drag a clipboard item straight into the agents composer
- **New:** SSH connections and launcher improvements
- **Improved:** Dragging floating tools is smoother and the clipboard loads faster

## 0.3.1 — 2026-08-09

- **Improved:** Better agents experience and more polish in the overlay
- **Fixed:** Clicks no longer get lost on the overlay's floating tools

## 0.3.0 — 2026-08-06

- **New:** Redesigned interface with a new liquid-style pill
- **New:** Global search and settings reorganized into sections
- **New:** AI agents in the pill, starting with Claude Code, with permissions and slash commands
- **New:** Multiple notes in Texts and selectable sounds per action
- **New:** A welcome flow with a final step that shows you how to use the app
- **New:** Dedicated Windows installer

## 0.2.0 — 2026-07-19

- **New:** Clipboard history in the pill, which flies to you when summoned
