/// macOS permission checks. Microphone access is prompted automatically by the
/// system when the first input stream opens (Info.plist has the usage string).
/// Accessibility is required for posting the Cmd+V event — prompt once at startup.

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(
            options: core_foundation::dictionary::CFDictionaryRef,
        ) -> bool;
    }

    pub fn accessibility_trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn prompt_accessibility() -> bool {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let options =
            CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
    }
}

pub fn ensure_accessibility_prompt() {
    #[cfg(target_os = "macos")]
    {
        if macos::accessibility_trusted() {
            tracing::info!("accessibility permission: granted");
        } else {
            tracing::warn!(
                "accessibility permission missing — prompting (needed to paste text)"
            );
            macos::prompt_accessibility();
        }
    }
}

pub fn accessibility_ok() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::accessibility_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
