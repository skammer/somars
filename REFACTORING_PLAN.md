# Somars Refactoring Plan

## Critical Issues

### 1. Deprecated Modules Still Present
**Location**: `src/keyboard.rs`, `src/ui.rs`, `src/audio_monitor.rs`

**Issue**: These modules are commented out in `main.rs` but still exist and import code that references them. The `keyboard.rs` module is 383 lines of code that duplicates logic already in `app.rs`. The `ui.rs` module (593 lines) duplicates component rendering logic. The `audio_monitor.rs` module depends on the deprecated `keyboard.rs`.

**Impact**: Dead code maintenance burden, confusion for new contributors.

**Fix**: Delete these three files entirely. Remove their commented-out declarations from `main.rs`.

---

### 2. Magic Component Indices
**Location**: `app.rs:185`, `app.rs:363-368`, `app.rs:382-388`, `app.rs:401-408`, etc.

**Issue**: Repeated numeric indices to access components:
```rust
components.get_mut(0)  // StationList
components.get_mut(1)  // NowPlaying
components.get_mut(2)  // History
components.get_mut(3)  // Help
components.get_mut(4)  // BottomControls
```

**Impact**: Fragile code - adding/removing components breaks all indices. Hard to read.

**Fix**: Define constants at module level:
```rust
const COMPONENT_STATION_LIST: usize = 0;
const COMPONENT_NOW_PLAYING: usize = 1;
const COMPONENT_HISTORY: usize = 2;
const COMPONENT_HELP: usize = 3;
const COMPONENT_BOTTOM_CONTROLS: usize = 4;
```

Or better: use an enum with `From<usize>` implementation.

---

### 3. Excessive Clone on Station Data
**Location**: `app.rs:625`, `app.rs:364`, `app.rs:382`, etc.

**Issue**: Frequent `.clone()` on `Station` structs:
```rust
if let Some(station) = self.stations.get(self.selected_station).cloned() {
    let _ = now_playing.update(Action::SetSelectedStation(Some(station)));
}
```

**Impact**: Unnecessary allocations, performance degradation.

**Fix**: Use `Arc<Station>` for shared ownership:
```rust
// In station.rs
#[derive(Clone)]
pub struct Station(Arc<InnerStation>);

struct InnerStation {
    pub id: String,
    pub title: String,
    // ... all fields
}
```

Or: pass station ID and let components fetch data when needed.

---

### 4. Noisy Logging in Hot Path
**Location**: `app.rs:281-283`

**Issue**: Logs every action at INFO level:
```rust
while let Ok(action) = self.action_rx.try_recv() {
    if action != Action::Tick && action != Action::Render {
        info!("{:?}", action);  // Floods logs
    }
}
```

**Impact**: Log spam, makes debugging harder, performance impact.

**Fix**: Change to `debug!` level:
```rust
debug!(?action);
```

---

### 5. Dead Code Allow Directives
**Location**: `src/audio/playback.rs:1`, `src/audio/stream.rs:1`, `src/audio/recovery.rs:1`, `src/audio/manager.rs:1`, `src/event.rs:1`, `src/tui.rs:1`, `src/i18n.rs:1`

**Issue**: `#![allow(dead_code)]` at module level indicates unused code that was left behind.

**Fix**: Either remove the directive and fix warnings, or delete truly unused code.

---

### 6. Inconsistent Error Handling
**Issue**: Mix of error types:
- `color_eyre::eyre::Result<T>` in `app.rs`, `main.rs`
- `anyhow::Result<T>` in some audio modules
- `thiserror` for `AppError` in `error.rs`
- Custom `AudioError` in `audio/types.rs`

**Impact**: Complexity, inconsistency, difficulty in error propagation.

**Fix**: Standardize on `color_eyre::eyre::Result<T>` for all public APIs. Convert internal errors using `.context()`.

---

### 7. Magic Numbers Throughout
**Locations**:
- `app.rs:796` - `0.05` for volume increment
- `app.rs:172-173` - `4.0` tick_rate, `60.0` frame_rate
- `main.rs:227-228` - Same rates passed to App::new

**Fix**: Extract to constants:
```rust
const DEFAULT_TICK_RATE: f64 = 4.0;
const DEFAULT_FRAME_RATE: f64 = 60.0;
const VOLUME_INCREMENT: f32 = 0.05;
const MAX_VOLUME: f32 = 2.0;
const MIN_VOLUME: f32 = 0.0;
```

---

### 8. Duplicate Code in Action Handling
**Location**: `app.rs:359-465`

**Issue**: Same pattern repeated for `TuneStation`, `TuneNext`, `TunePrev`, `StationUp`, `StationDown`:
```rust
// Update NowPlaying component with the selected station details
if let Some(now_playing) = self.components.get_mut(1) {
    if let Some(station) = self.stations.get(self.selected_station).cloned() {
        let _ = now_playing.update(Action::SetSelectedStation(Some(station)));
    } else {
        let _ = now_playing.update(Action::SetSelectedStation(None));
    }
}
```

**Fix**: Extract to helper method:
```rust
fn sync_station_to_now_playing(&mut self) {
    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
        let station = self.stations.get(self.selected_station).cloned();
        let _ = now_playing.update(Action::SetSelectedStation(station));
    }
}
```

---

## Implementation Order

### Phase 1: Cleanup (Low Risk)
1. Delete deprecated modules (`keyboard.rs`, `ui.rs`, `audio_monitor.rs`)
2. Remove commented-out module declarations in `main.rs`
3. Add component index constants to `app.rs`
4. Extract magic numbers to constants

### Phase 2: Reduce Noise
5. Change action logging from `info!` to `debug!`
6. Remove dead_code allow directives where code is actually used

### Phase 3: Performance
7. Replace `.clone()` with `Arc<Station>` (requires more testing)
8. Standardize error handling approach

---

## Files to Delete
- `src/keyboard.rs` (383 lines - duplicated logic)
- `src/ui.rs` (593 lines - duplicated UI rendering)
- `src/audio_monitor.rs` (28 lines - depends on keyboard.rs)

**Total: ~1000 lines of dead code removed**
