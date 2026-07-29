/// Remember which app was frontmost when dictation started and re-activate it
/// before pasting, so focus theft (overlays, pill clicks) cannot break insertion.

#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<(i32, Option<String>)> {
    use objc2_app_kit::NSWorkspace;
    let ws = NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    let pid = app.processIdentifier();
    let bundle = app.bundleIdentifier().map(|s| s.to_string());
    Some((pid, bundle))
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<(i32, Option<String>)> {
    None
}

/// Bring the app with `pid` back to front. Returns true when activation was requested.
#[cfg(target_os = "macos")]
pub fn activate(pid: i32) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    match NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        Some(app) => {
            #[allow(deprecated)]
            app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
            true
        }
        None => false,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate(_pid: i32) -> bool {
    false
}
