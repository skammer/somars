//! Runs AppKit on the process main thread while the TUI uses Tokio on a worker.

use color_eyre::eyre::{eyre, Result};
use dispatch::Queue;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::NSPoint;

struct StopAppOnDrop;

impl Drop for StopAppOnDrop {
    fn drop(&mut self) {
        Queue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let app = NSApplication::sharedApplication(mtm);
            app.stop(None);
            if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::ApplicationDefined,
                NSPoint::default(),
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                0,
                0,
                0,
            ) {
                app.postEvent_atStart(&event, true);
            }
        });
    }
}

pub fn run<F>(worker: F) -> Result<()>
where
    F: FnOnce() -> Result<()> + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or_else(|| eyre!("AppKit requires the main thread"))?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();

    let worker = std::thread::Builder::new()
        .name("somars-runtime".to_string())
        .spawn(move || {
            let _stop_app = StopAppOnDrop;
            worker()
        })
        .map_err(|error| eyre!("Failed to start application runtime: {error}"))?;

    app.run();

    worker
        .join()
        .map_err(|_| eyre!("Application runtime thread panicked"))?
}
