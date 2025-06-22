# PRD: Spotify Account Integration and Single Disc Playback

## Introduction/Overview

The 6-Disc-Changer is a web application that recreates the nostalgic experience of using a 90s CD player, but with modern Spotify integration. This feature enables users to authenticate with their Spotify account and play albums through a traditional CD player interface, complete with authentic controls and visual feedback. This initial implementation focuses on single-disc functionality as a proof of concept for the full 6-disc changer experience.

## Goals

1. Enable users to authenticate using their existing Spotify account (no separate registration required)
2. Allow users to select and play one Spotify album using traditional CD player controls
3. Provide an authentic 90s CD player experience with realistic UI and interaction patterns
4. Establish the foundation for future 6-disc changer functionality
5. Create a "CD Book" system for saving user's album selections

## User Stories

1. **As a nostalgic music lover**, I want to log in with my Spotify account so that I can access my music library through a retro interface.

2. **As a user**, I want to browse and select an album from Spotify to "load" into my CD player so that I can play it with classic controls.

3. **As a user**, I want to control playback using traditional CD player buttons (Play, Pause, Stop, etc.) so that I can relive the tactile experience of using a physical CD player.

4. **As a user**, I want to "eject" the current disc and load a new album so that I can change what I'm listening to (even if it takes effort like a real CD player).

5. **As a user**, I want my selected albums saved in a "CD Book" so that I can quickly access my favorite albums in future sessions.

## Functional Requirements

1. **Authentication**
   - 1.1. The system must provide a "Login with Spotify" button on the landing page
   - 1.2. The system must authenticate users via Spotify OAuth 2.0
   - 1.3. The system must request appropriate Spotify scopes: user profile, playback control, and search capabilities

2. **Album Selection**
   - 2.1. The system must allow users to search for albums on Spotify
   - 2.2. The system must display album results with cover art, artist name, and album title
   - 2.3. The system must allow users to select one album to "load" into the CD player

3. **CD Player Interface**
   - 3.1. The system must display a visual representation of a 90s CD player
   - 3.2. The system must show the currently loaded album's information
   - 3.3. The system must display traditional CD player status indicators (track number, time elapsed, play status)

4. **Playback Controls**
   - 4.1. The system must implement Play/Pause functionality
   - 4.2. The system must implement Stop functionality (stops playback and returns to track 1)
   - 4.3. The system must implement Next Track and Previous Track navigation
   - 4.4. The system must implement Shuffle mode
   - 4.5. The system must implement Intro Scan (plays first 10 seconds of each track)
   - 4.6. All playback must occur through Spotify's player (not direct streaming)

5. **Disc Management**
   - 5.1. The system must implement an "Eject" button that unloads the current album
   - 5.2. The eject process must be deliberately slow/cumbersome (e.g., animation, delay, multiple steps)
   - 5.3. The system must require the disc tray to be "open" before loading a new album

6. **CD Book Storage**
   - 6.1. The system must save all albums a user has added to their collection locally
   - 6.2. The system must display saved albums in a "CD Book" interface
   - 6.3. The system must persist the CD Book across sessions

7. **Error Handling**
   - 7.1. The system must detect when a user's Spotify subscription has expired
   - 7.2. The system must display CD titles but disable all playback functions if Spotify access is lost
   - 7.3. The system must detect when an album becomes unavailable on Spotify
   - 7.4. The system must display a humorous "CD is scratched" message for unavailable albums

## Non-Goals (Out of Scope)

1. Mobile or desktop application support (web only)
2. Multiple disc functionality (limited to single disc for this phase)
3. Direct audio streaming (all playback through Spotify)
4. Custom playlists or individual track selection
5. Social features or sharing capabilities
6. Offline playback
7. Integration with other music services

## Design Considerations

- **Visual Design**: The interface should authentically recreate a 90s CD player aesthetic with:
  - Brushed metal or plastic textures
  - LCD-style display for track information
  - Physical-looking buttons with pressed states
  - Red/green LED indicators
  - Realistic CD tray animation

- **CD Book Design**: Should resemble a physical CD wallet/book with:
  - Page-turning animations
  - Sleeve pockets for album artwork
  - Capacity indicators

- **Feedback**: All actions should provide visual and temporal feedback similar to physical hardware:
  - Button press animations
  - Mechanical sounds (optional)
  - Realistic delays for disc loading/ejecting

## Technical Considerations

1. **Spotify Web API Integration**
   - Implement OAuth 2.0 flow for authentication
   - Use Spotify Web API for search and metadata
   - Use Spotify Web Playback SDK for browser-based playback control

2. **State Management**
   - Track current playback state (playing, paused, stopped)
   - Maintain current track position and total duration
   - Store shuffle mode and intro scan status

3. **Local Storage**
   - Implement localStorage or IndexedDB for CD Book persistence
   - Store album IDs, metadata, and artwork URLs
   - Handle storage limits gracefully

4. **Performance**
   - Lazy load album artwork
   - Implement debouncing for search functionality
   - Cache frequently accessed album data

## Success Metrics

1. **User Engagement**
   - 75% of authenticated users load at least one album
   - Average session duration of 15+ minutes
   - 50% of users return within one week

2. **Feature Adoption**
   - 80% of users try the eject/load mechanism at least once
   - 60% of users add multiple albums to their CD Book
   - 40% of users engage with intro scan feature

3. **Technical Performance**
   - Page load time under 3 seconds
   - Playback commands execute within 500ms
   - Zero critical errors in production

## Open Questions

1. Should we implement keyboard shortcuts for power users (e.g., spacebar for play/pause)?
2. Do we want to add authentic CD player sounds (mechanical whirring, clicking)?
3. Should the intro scan duration be configurable (5, 10, 15 seconds)?
4. How many albums should the CD Book support before pagination is needed?
5. Should we implement a "remember last played album" feature for returning users?
6. Do we want to support album pre-loading while the tray is closed (realistic vs. convenient)?
7. Should we add visual CD spinning animation during playback?