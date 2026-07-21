use crate::{action::Action, station::Station, PlaybackState};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::ffi::c_void;
use std::io;
use std::sync::mpsc;
use std::time::Duration;
use tracing::warn;
use windows::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, TranslateMessage, MSG, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_QUIT,
    WNDCLASSEXW,
};

#[derive(Debug)]
enum Update {
    PlaybackState(PlaybackState),
    Station {
        station: Box<Station>,
        track_title: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct MediaSessionHandle {
    update_tx: mpsc::Sender<Update>,
}

impl MediaSessionHandle {
    pub fn start(action_tx: tokio::sync::mpsc::UnboundedSender<Action>, _volume: f32) -> Self {
        let (update_tx, update_rx) = mpsc::channel();
        if let Err(error) = std::thread::Builder::new()
            .name("somars-media-session".to_string())
            .spawn(move || {
                if let Err(error) = run(action_tx, update_rx) {
                    warn!(%error, "Windows media-session integration unavailable");
                }
            })
        {
            warn!(%error, "failed to start Windows media-session thread");
        }
        Self { update_tx }
    }

    pub fn set_playback_state(&self, state: PlaybackState) {
        let _ = self.update_tx.send(Update::PlaybackState(state));
    }

    pub fn set_station(&self, station: Station) {
        let _ = self.update_tx.send(Update::Station {
            station: Box::new(station),
            track_title: None,
        });
    }

    pub fn set_track_title(&self, station: Station, title: String) {
        let _ = self.update_tx.send(Update::Station {
            station: Box::new(station),
            track_title: Some(title),
        });
    }

    pub fn set_volume(&self, _volume: f32) {}
}

fn run(
    action_tx: tokio::sync::mpsc::UnboundedSender<Action>,
    update_rx: mpsc::Receiver<Update>,
) -> Result<(), String> {
    let window = HiddenWindow::new()?;
    let config = PlatformConfig {
        dbus_name: "somars",
        display_name: "somars",
        hwnd: Some(window.handle.0 as *mut c_void),
    };
    let mut controls = MediaControls::new(config).map_err(|error| error.to_string())?;
    controls
        .attach(move |event| handle_control_event(&action_tx, event))
        .map_err(|error| error.to_string())?;

    loop {
        match update_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(update) => {
                apply_update(&mut controls, update)?;
                for update in update_rx.try_iter() {
                    apply_update(&mut controls, update)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if !pump_event_queue() {
            break;
        }
    }

    Ok(())
}

fn apply_update(controls: &mut MediaControls, update: Update) -> Result<(), String> {
    match update {
        Update::PlaybackState(state) => controls
            .set_playback(match state {
                PlaybackState::Playing => MediaPlayback::Playing { progress: None },
                PlaybackState::Paused => MediaPlayback::Paused { progress: None },
                PlaybackState::Stopped => MediaPlayback::Stopped,
            })
            .map_err(|error| error.to_string()),
        Update::Station {
            station,
            track_title,
        } => controls
            .set_metadata(MediaMetadata {
                title: Some(track_title.as_deref().unwrap_or(&station.title)),
                album: Some(&station.title),
                artist: (!station.dj.is_empty()).then_some(station.dj.as_str()),
                cover_url: (!station.image.is_empty()).then_some(station.image.as_str()),
                duration: None,
            })
            .map_err(|error| error.to_string()),
    }
}

fn handle_control_event(
    action_tx: &tokio::sync::mpsc::UnboundedSender<Action>,
    event: MediaControlEvent,
) {
    let action = match event {
        MediaControlEvent::Play => Some(Action::Play),
        MediaControlEvent::Pause => Some(Action::Pause),
        MediaControlEvent::Toggle => Some(Action::TogglePause),
        MediaControlEvent::Next => Some(Action::TuneNext),
        MediaControlEvent::Previous => Some(Action::TunePrev),
        MediaControlEvent::Stop => Some(Action::Stop),
        _ => None,
    };
    if let Some(action) = action {
        let _ = action_tx.send(action);
    }
}

struct HiddenWindow {
    handle: HWND,
}

impl HiddenWindow {
    fn new() -> Result<Self, String> {
        let class_name = w!("SomarsMediaSessionWindow");
        let instance = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
        let window_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: instance,
            lpszClassName: class_name,
            lpfnWndProc: Some(window_proc),
            ..Default::default()
        };

        if unsafe { RegisterClassExW(&window_class) } == 0 {
            return Err(format!(
                "failed to register media-session window: {}",
                io::Error::last_os_error()
            ));
        }

        let handle = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("somars"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                instance,
                None,
            )
        };
        if handle.0 == 0 {
            return Err(format!(
                "failed to create media-session window: {}",
                io::Error::last_os_error()
            ));
        }

        Ok(Self { handle })
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn pump_event_queue() -> bool {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_QUIT {
                return false;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    true
}
