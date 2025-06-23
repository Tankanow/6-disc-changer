## Notes

- Unit tests should be placed inside the same code files they are testing under a module called `test`.
- Use `cargo test [optional/path/to/test/file]` to run tests. Running without a path executes all tests found by the cargo configuration.

## Tasks

- [ ] 1.0 Set up Spotify OAuth Authentication
  - [x] 1.1 Register application with Spotify Developer Dashboard and obtain client ID/secret
  - [x] 1.2 Implement OAuth 2.0 authorization code flow with PKCE
  - [ ] 1.3 Create login page with "Login with Spotify" button
  - [ ] 1.4 Handle OAuth callback and token exchange
  - [ ] 1.5 Implement token refresh mechanism for expired access tokens
  - [ ] 1.6 Create authentication state management (logged in/out states)
  - [ ] 1.7 Request required scopes: user-read-private, user-read-email, streaming, user-modify-playback-state, user-read-playback-state

- [ ] 2.0 Implement Album Search and Selection Interface
  - [ ] 2.1 Create search input component with debouncing
  - [ ] 2.2 Integrate Spotify Web API search endpoint for albums
  - [ ] 2.3 Build search results display with album artwork, artist, and title
  - [ ] 2.4 Implement album selection handler
  - [ ] 2.5 Create loading states and error handling for search
  - [ ] 2.6 Add pagination or infinite scroll for search results
  - [ ] 2.7 Cache search results for performance

- [ ] 3.0 Create CD Player UI Components
  - [ ] 3.1 Design main CD player container with 90s aesthetic
  - [ ] 3.2 Create LCD-style display component for track info (track number, time, status)
  - [ ] 3.3 Build CD tray component with open/close animations
  - [ ] 3.4 Implement visual feedback for button presses (3D pressed effect)
  - [ ] 3.5 Add LED indicators for power and play status
  - [ ] 3.6 Create CD spinning animation for playback visualization
  - [ ] 3.7 Style all components with brushed metal/plastic textures

- [ ] 4.0 Implement Playback Controls and Spotify SDK Integration
  - [ ] 4.1 Initialize Spotify Web Playback SDK in the browser
  - [ ] 4.2 Create playback device and register with Spotify
  - [ ] 4.3 Implement Play/Pause toggle functionality
  - [ ] 4.4 Create Stop function (pause and seek to beginning)
  - [ ] 4.5 Build Next/Previous track navigation
  - [ ] 4.6 Implement Shuffle mode with state persistence
  - [ ] 4.7 Create Intro Scan feature (10-second preview per track)
  - [ ] 4.8 Handle eject mechanism with deliberate delays
  - [ ] 4.9 Implement disc loading with tray state validation
  - [ ] 4.10 Add playback error handling (expired token, lost connection)
  - [ ] 4.11 Create "CD is scratched" error for unavailable albums

- [ ] 5.0 Build CD Book Storage System
  - [ ] 5.1 Design CD Book UI with page-turning interface
  - [ ] 5.2 Implement local storage schema for album data
  - [ ] 5.3 Create add/remove album functions for CD Book
  - [ ] 5.4 Build album grid/list view with artwork
  - [ ] 5.5 Implement quick-load from CD Book to player
  - [ ] 5.6 Add storage limit handling and user notifications
  - [ ] 5.7 Create data migration strategy for future updates
  - [ ] 5.8 Handle offline mode (show titles only when Spotify unavailable)
