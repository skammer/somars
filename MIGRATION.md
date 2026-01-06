# Somars Architecture Migration Plan

This document outlines a plan to migrate somars to follow the best practices demonstrated in the example ratatui project (`example/component-generated/`).

## Progress Tracking

- [x] **Phase 1: Foundation** - COMPLETED ✅
  - [x] Add dependencies (color-eyre, strum, tokio-util)
  - [x] Create src/action.rs
  - [x] Create src/event.rs
  - [x] Create src/tui.rs
  - [x] Create src/components.rs
  - [x] Update error handling for color-eyre
  - [x] Update main.rs with new module declarations

- [x] **Phase 2: Component Extraction** - COMPLETED ✅
  - [x] Create src/components/mod.rs
  - [x] Extract StationList component
  - [x] Extract NowPlaying component
  - [x] Extract History component
  - [x] Extract Help component
  - [x] Extract BottomControls component

- [x] **Phase 3: App Structure** - COMPLETED ✅
  - [x] Create src/app.rs
  - [x] Implement event loop
  - [x] Implement action handling
  - [x] Implement render method

- [x] **Phase 4: Integration** - COMPLETED ✅
  - [x] Update main.rs to use new App
  - [x] Connect audio playback
  - [x] Connect metadata
  - [x] Remove old event loop code

- [x] **Phase 5: Optimization** - COMPLETED ✅
  - [x] Implement efficient rendering (separate tick 4/sec and render 60fps rates)
  - [x] Build and verify compilation

## Problem Statement

The current somars codebase has several architectural issues:
1. **Monolithic UI rendering** - Single 540+ line `ui()` function in `ui.rs`
2. **Mixed concerns** - Event handling, rendering, and state management intertwined
3. **High CPU usage** - Inefficient rendering loop (30% CPU on large windows)
4. **Poor modularity** - Difficult to add new features or modify existing ones
5. **Manual terminal management** - Terminal cleanup scattered throughout main.rs

## Target Architecture

The example project demonstrates a **component-based, event-driven architecture** with:
- **Component trait** - Uniform interface for all UI elements
- **Action enum** - Command pattern for state changes
- **Tui wrapper** - Centralized terminal lifecycle management
- **Async event loop** - Efficient event handling via tokio
- **Channel-based communication** - Loose coupling between components

## Migration Strategy

### Phase 1: Foundation (No Breaking Changes)

**Goal**: Set up the new architecture alongside existing code.

#### 1.1 Add Dependencies

Update `Cargo.toml`:
```toml
# New dependencies
color-eyre = "0.6"        # Better error handling
strum = "0.26"            # Enum derives
strum_macros = "0.26"
tokio-util = { version = "0.7", features = ["sync"] }
signal-hook = "0.3"       # Already present
```

#### 1.2 Create Core Modules

Create new files (keep existing ones intact):

**`src/action.rs`** - Action enum for all commands:
```rust
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum Action {
    // System
    Tick,
    Render,
    Resize(u16, u16),
    Quit,
    Error(String),

    // Playback
    Play,
    Stop,
    TogglePause,
    Pause,
    Resume,

    // Navigation
    StationUp,
    StationDown,
    ScrollHistoryUp,
    ScrollHistoryDown,

    // Volume
    VolumeUp,
    VolumeDown,
    SetVolume(f32),

    // Station
    SelectStation(usize),
    TuneStation(String),

    // UI
    ToggleHelp,
}
```

**`src/event.rs`** - Event types:
```rust
use crossterm::event::{KeyEvent, MouseEvent};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Init,
    Quit,
    Error,
    Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}
```

**`src/tui.rs`** - Terminal management (adapt from example):
```rust
// Copy and adapt from example/component-generated/src/tui.rs
// Key changes:
// - Use color_eyre::Result instead of anyhow
// - Keep signal handling for SIGTSTP (suspend/resume)
```

**`src/components.rs`** - Component trait definition:
```rust
// Copy from example/component-generated/src/components.rs
// Use color_eyre::Result
```

#### 1.3 Update Error Handling

Modify `src/error.rs` to integrate `color-eyre`:
```rust
pub type Result<T> = color_eyre::Result<T>;
```

Update `src/main.rs`:
```rust
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    // ... existing code
}
```

### Phase 2: Component Extraction

**Goal**: Extract UI elements into components.

#### 2.1 Create Component Modules

Create `src/components/` directory with:

**`src/components/mod.rs`**:
```rust
pub mod station_list;
pub mod now_playing;
pub mod history;
pub mod help;
pub mod bottom_controls;

pub use station_list::StationList;
pub use now_playing::NowPlaying;
pub use history::History;
pub use help::Help;
pub use bottom_controls::BottomControls;
```

**`src/components/station_list.rs`** - Extract from `ui.rs`:
```rust
pub struct StationList {
    stations: Vec<Station>,
    selected: ListState,
    action_tx: UnboundedSender<Action>,
}

impl Component for StationList {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        // Extracted from render_station_list()
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<Option<Action>> {
        match key.code {
            KeyCode::Up => Some(Action::StationUp),
            KeyCode::Down => Some(Action::StationDown),
            KeyCode::Enter => Some(Action::Play),
            _ => None,
        }
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        match action {
            Action::StationUp => self.move_up(),
            Action::StationDown => self.move_down(),
            Action::SelectStation(idx) => self.select(idx),
            _ => {}
        }
        Ok(None)
    }
}
```

**`src/components/now_playing.rs`**:
```rust
pub struct NowPlaying {
    active_station: Option<String>,
    playback_state: PlaybackState,
    playback_frame_index: usize,
    playback_frames: Vec<&'static str>,
}

impl Component for NowPlaying {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        // Extracted from render_now_playing()
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        match action {
            Action::Tick => self.advance_frame(),
            Action::Play => self.playback_state = PlaybackState::Playing,
            Action::Stop => self.playback_state = PlaybackState::Stopped,
            _ => {}
        }
        Ok(None)
    }
}
```

**`src/components/history.rs`**:
```rust
pub struct History {
    messages: VecDeque<HistoryMessage>,
    scroll_state: ListState,
    cache: HashMap<usize, Vec<String>>,
    cache_valid: bool,
    last_width: u16,
}

impl Component for History {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        // Extracted from render_history()
        // Keep caching logic
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        match action {
            Action::ScrollHistoryUp => self.scroll_up(),
            Action::ScrollHistoryDown => self.scroll_down(),
            Action::Error(msg) => self.add_message(msg, MessageType::Error),
            _ => {}
        }
        Ok(None)
    }
}
```

**`src/components/help.rs`**:
```rust
pub struct Help {
    visible: bool,
}

impl Component for Help {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        // Extracted from render_help_popup()
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<Option<Action>> {
        if self.visible {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => Some(Action::ToggleHelp),
                _ => None,
            }
        } else {
            match key.code {
                KeyCode::Char('?') => Some(Action::ToggleHelp),
                _ => None,
            }
        }
    }
}
```

**`src/components/bottom_controls.rs`**:
```rust
pub struct BottomControls {
    volume: f32,
    total_played: Duration,
    log_level: u8,
}

impl Component for BottomControls {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        // Extracted from render_bottom_controls()
    }
}
```

### Phase 3: App Structure

**Goal**: Create the main App struct with event loop.

#### 3.1 Create `src/app.rs`

```rust
use crate::{action::Action, components::Component, tui::Tui, event::Event};

pub struct App {
    // Components
    components: Vec<Box<dyn Component>>,

    // Shared state
    config: Config,
    stations: Vec<Station>,
    active_station: Option<usize>,

    // Audio
    audio_manager: audio::AudioManager,
    metadata_tx: UnboundedSender<audio::MetadataEvent>,

    // Channels
    action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,

    // State
    should_quit: bool,
    mode: Mode,
}

impl App {
    pub fn new(tick_rate: f64, frame_rate: f64) -> color_eyre::Result<Self> {
        let (action_tx, action_rx) = unbounded_channel();

        let components: Vec<Box<dyn Component>> = vec![
            Box::new(StationList::new(action_tx.clone())),
            Box::new(NowPlaying::new()),
            Box::new(History::new()),
            Box::new(Help::new()),
            Box::new(BottomControls::new()),
        ];

        Ok(Self {
            components,
            config: Config::load_or_default(),
            stations: Vec::new(),
            active_station: None,
            audio_manager: audio::AudioManager::new(),
            metadata_tx: create_metadata_channel(),
            action_tx,
            action_rx,
            should_quit: false,
            mode: Mode::Normal,
        })
    }

    pub async fn run(&mut self) -> color_eyre::Result<()> {
        let mut tui = Tui::new()?
            .tick_rate(4.0)   // 4 ticks/sec
            .frame_rate(60.0); // 60 fps

        tui.enter()?;

        // Initialize components
        for component in self.components.iter_mut() {
            component.register_action_handler(self.action_tx.clone())?;
            component.register_config_handler(self.config.clone())?;
            component.init(tui.size()?)?;
        }

        // Main event loop
        loop {
            self.handle_events(&mut tui).await?;
            self.handle_actions(&mut tui)?;

            if self.should_quit {
                break;
            }
        }

        tui.exit()?;
        Ok(())
    }

    async fn handle_events(&mut self, tui: &mut Tui) -> color_eyre::Result<()> {
        let Some(event) = tui.next_event().await else {
            return Ok(());
        };

        // Convert events to actions
        match event {
            Event::Quit => self.action_tx.send(Action::Quit)?,
            Event::Tick => self.action_tx.send(Action::Tick)?,
            Event::Render => self.action_tx.send(Action::Render)?,
            Event::Resize(w, h) => self.action_tx.send(Action::Resize(w, h))?,
            Event::Key(key) => self.handle_key_event(key)?,
            _ => {}
        }

        // Forward events to components
        for component in self.components.iter_mut() {
            if let Some(action) = component.handle_events(Some(event.clone()))? {
                self.action_tx.send(action)?;
            }
        }

        Ok(())
    }

    fn handle_actions(&mut self, tui: &mut Tui) -> color_eyre::Result<()> {
        while let Ok(action) = self.action_rx.try_recv() {
            // Handle app-level actions
            match &action {
                Action::Quit => self.should_quit = true,
                Action::Render => self.render(tui)?,
                Action::Resize(w, h) => tui.resize(Rect::new(0, 0, *w, *h))?,
                _ => {}
            }

            // Forward to components
            for component in self.components.iter_mut() {
                if let Some(new_action) = component.update(action.clone())? {
                    self.action_tx.send(new_action)?;
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> color_eyre::Result<()> {
        tui.draw(|frame| {
            let layout = self.calculate_layout(frame.area());

            for (i, component) in self.components.iter_mut().enumerate() {
                let area = match i {
                    0 => layout.left_panel,      // Station list
                    1 => layout.right_top,       // Now playing
                    2 => layout.right_bottom,    // History
                    3 => frame.area(),           // Help (overlay)
                    4 => layout.bottom,          // Controls
                    _ => frame.area(),
                };
                let _ = component.draw(frame, area);
            }
        })?;
        Ok(())
    }
}
```

### Phase 4: Integration

**Goal**: Wire everything together and remove old code.

#### 4.1 Update `src/main.rs`

```rust
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    // Setup
    color_eyre::install()?;
    logging::init_logging();

    let cli = Cli::parse();
    let mut config = Config::load_or_default();

    // Create and run app
    let mut app = App::new(4.0, 60.0)?;
    app.run().await?;

    Ok(())
}
```

#### 4.2 Remove Old Files

After successful migration:
- Remove `src/ui.rs` (functionality moved to components)
- Remove `src/keyboard.rs` (moved to component handle_key_event)
- Remove `src/control.rs` (replaced by Action enum)

### Phase 5: Optimization

**Goal**: Reduce CPU usage.

#### 5.1 Implement Efficient Rendering

The example project uses separate tick and render rates:
- **Tick rate**: 4/sec (logic updates, animations)
- **Frame rate**: 60 fps (max rendering rate)

Key optimizations:
1. **Don't render on every input** - Only render on Action::Render
2. **Cache rendered output** - Components can cache their rendered state
3. **Dirty tracking** - Only redraw changed components

#### 5.2 Reduce Redraw Frequency

```rust
// In Tui event loop
let mut tick_interval = interval(Duration::from_secs_f64(1.0 / tick_rate));
let mut render_interval = interval(Duration::from_secs_f64(1.0 / frame_rate));

tokio::select! {
    _ = tick_interval.tick() => Event::Tick,
    _ = render_interval.tick() => Event::Render,
    // ... handle other events
}
```

This ensures:
- Background tasks run at tick rate (4/sec)
- Rendering limited to frame rate (60 fps max)
- Input processed immediately (no waiting)

## Migration Steps (Ordered)

### Step 1: Add new dependencies (5 min)
- Update Cargo.toml
- Run `cargo build`

### Step 2: Create foundation modules (30 min)
- src/action.rs
- src/event.rs
- Update src/error.rs for color_eyre

### Step 3: Create Tui wrapper (1 hour)
- Copy from example
- Adapt for somars needs
- Test terminal enter/exit

### Step 4: Create Component trait (30 min)
- Copy from example
- Create components module structure

### Step 5: Extract one component (1 hour)
- Start with StationList
- Implement Component trait
- Test in isolation

### Step 6: Extract remaining components (3 hours)
- NowPlaying
- History
- Help
- BottomControls

### Step 7: Create App struct (2 hours)
- Basic structure
- Event loop
- Action handling

### Step 8: Wire everything together (2 hours)
- Update main.rs
- Connect audio playback
- Connect metadata

### Step 9: Testing (2 hours)
- Manual testing of all features
- Fix bugs
- Ensure feature parity

### Step 10: Cleanup (1 hour)
- Remove old files
- Update documentation
- Final polish

**Total Estimated Time**: 13-15 hours

## Benefits of Migration

1. **Reduced CPU Usage**: Efficient event loop eliminates unnecessary redraws
2. **Better Modularity**: Easy to add/remove features
3. **Improved Testability**: Components can be tested in isolation
4. **Clearer Architecture**: Separation of concerns
5. **Easier Maintenance**: Changes localized to specific components
6. **Better Error Handling**: color-eyre provides rich error context
7. **Graceful Terminal Management**: RAII ensures proper cleanup

## Compatibility

This migration maintains full feature parity:
- All keyboard shortcuts work the same
- Same visual appearance
- Same audio functionality
- Same configuration options
- UDP control still works

## Risk Mitigation

1. **Incremental approach**: Each phase can be tested independently
2. **Keep old code**: Don't delete until replacement is verified
3. **Feature flags**: Can toggle between old/new implementation
4. **Committed refactoring**: Git allows easy rollback
5. **Testing**: Manual testing at each step

## References

- Example project: `example/component-generated/`
- Ratatui book: https://ratatui.rs/
- Tokio documentation: https://tokio.rs/
