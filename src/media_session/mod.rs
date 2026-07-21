//! Native desktop media-session integration.

#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(all(target_os = "macos", not(test)))]
mod macos;
#[cfg(all(
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(test)
))]
mod unsupported;
#[cfg(all(target_os = "windows", not(test)))]
mod windows;

#[cfg(any(target_os = "linux", test))]
pub use linux::MediaSessionHandle;
#[cfg(all(target_os = "macos", not(test)))]
pub use macos::MediaSessionHandle;
#[cfg(all(
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(test)
))]
pub use unsupported::MediaSessionHandle;
#[cfg(all(target_os = "windows", not(test)))]
pub use windows::MediaSessionHandle;
