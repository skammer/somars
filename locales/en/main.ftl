# General UI
app-title = SomaFM Player
app-description = A rusty soma.fm player

# Main UI
stations = Stations
history = History
loading = Loading
loading-stations = Loading stations...
no-station-selected = No station selected
volume = Volume
playback-time = Playback time

# Station info
station-id = ID
station-title = Title
station-genre = Genre
station-dj = DJ

# Playback states
playing = Playing
paused = Paused
stopped = Stopped

# Controls
controls-quit = Quit
controls-play = Play
controls-stop = Stop
controls-start = Start
controls-pause = Pause
controls-volume = Volume
controls-help = Help

# Messages
connecting-to-stream = Connecting to stream...
playback-started = Playback started
playback-error = Playback error: {$error}
starting-playback = Starting playback of {$station}
failed-playback = Failed to start playback
failed-audio-sink = Failed to lock audio sink
failed-decoder-construction = Failed to construct audio decoder: {$error}
stream-from = Initializing stream from: {$url}
got-response = Got response, starting stream...
bit-rate = Bit rate: {$rate}kbps
udp-starting = Starting UDP command listener on port {$port}
udp-error = UDP error: {$error}
station-not-found = Station ID not found: {$id}
auto-playing = Auto-playing station: {$id}
underrun-detected = Audio buffer underrun detected, restarting playback...

# Help screen
help-title = Help
help-keyboard = Keyboard Controls
help-enter = Play selected station
help-space = Stop/Start playback
help-volume = Adjust volume
help-arrows = Navigate stations
help-quit = Quit application
help-toggle-help = Toggle this help screen
help-cli = Command Line Arguments
help-log-level = Set log verbosity (1=minimal, 2=verbose)
help-station = Auto-play station with given ID on startup
help-listen = Enable UDP control listener
help-port = Set UDP port (default: 8069)
help-show-help = Show command line help
help-version = Show version information
help-broadcast = Send UDP command to network and exit
help-locale = Set the locale (en, ru)
help-close = Press ? to close this help screen
