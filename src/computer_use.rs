use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseStatus {
    pub platform: String,
    pub accessibility_supported: bool,
    pub permission_prompt_supported: bool,
    pub accessibility_trusted: bool,
    pub prompt_requested: bool,
    pub guidance: String,
}

impl ComputerUseStatus {
    pub fn ready(&self) -> bool {
        self.accessibility_supported && self.accessibility_trusted
    }
}

pub fn inspect_computer_use(prompt: bool) -> ComputerUseStatus {
    platform::inspect_computer_use(prompt)
}

pub fn frontmost_app_snapshot(max_items: usize) -> Result<String> {
    platform::frontmost_app_snapshot(max_items)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{ComputerUseStatus, Result};
    use anyhow::{Context, bail};
    use std::ffi::c_void;
    use std::process::Command;
    use std::ptr;

    type Boolean = u8;
    type CFIndex = isize;
    type CFDictionaryRef = *const c_void;
    type CFTypeRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> Boolean;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
        static kAXTrustedCheckOptionPrompt: CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: CFIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;
        fn CFRelease(cf: CFTypeRef);
    }

    pub fn inspect_computer_use(prompt: bool) -> ComputerUseStatus {
        let trusted = accessibility_trusted(prompt);
        ComputerUseStatus {
            platform: "macos".to_string(),
            accessibility_supported: true,
            permission_prompt_supported: true,
            accessibility_trusted: trusted,
            prompt_requested: prompt,
            guidance: if trusted {
                "Accessibility permission is granted. Native computer-use snapshots can inspect the frontmost app.".to_string()
            } else if prompt {
                "macOS should show an Accessibility permission prompt. If it does not, open System Settings > Privacy & Security > Accessibility and enable Crab or the launching terminal.".to_string()
            } else {
                "Run computer_use with action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility.".to_string()
            },
        }
    }

    pub fn frontmost_app_snapshot(max_items: usize) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use snapshot requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let max_items = max_items.clamp(1, 50);
        let script = [
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "set appPid to unix id of frontApp",
            "set output to \"frontmost_app: \" & appName & linefeed & \"pid: \" & appPid",
            "set output to output & linefeed & \"windows:\"",
            &format!("set maxItems to {max_items}"),
            "set itemIndex to 0",
            "repeat with w in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "set itemIndex to itemIndex + 1",
            "try",
            "set windowName to name of w",
            "set {x, y} to position of w",
            "set {wide, high} to size of w",
            "set output to output & linefeed & \"- index: \" & itemIndex & \", title: \" & quoted form of windowName & \", position: (\" & x & \", \" & y & \"), size: (\" & wide & \" x \" & high & \")\"",
            "on error errMsg",
            "set output to output & linefeed & \"- index: \" & itemIndex & \", error: \" & quoted form of errMsg",
            "end try",
            "end repeat",
            "return output",
            "end tell",
        ];

        let mut command = Command::new("osascript");
        for line in script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility snapshot")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility snapshot failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    }

    fn accessibility_trusted(prompt: bool) -> bool {
        if prompt {
            return accessibility_trusted_with_prompt();
        }
        // SAFETY: AXIsProcessTrusted has no preconditions and returns the current process trust state.
        unsafe { AXIsProcessTrusted() != 0 }
    }

    fn accessibility_trusted_with_prompt() -> bool {
        // SAFETY: The CoreFoundation dictionary only references constant CF objects that remain valid
        // for the duration of the call. The dictionary itself is released after AX reads it.
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt as *const c_void];
            let values = [kCFBooleanTrue as *const c_void];
            let options = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                ptr::null(),
                ptr::null(),
            );
            if options.is_null() {
                return AXIsProcessTrusted() != 0;
            }
            let trusted = AXIsProcessTrustedWithOptions(options) != 0;
            CFRelease(options as CFTypeRef);
            trusted
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{ComputerUseStatus, Result};
    use anyhow::bail;

    pub fn inspect_computer_use(prompt: bool) -> ComputerUseStatus {
        ComputerUseStatus {
            platform: std::env::consts::OS.to_string(),
            accessibility_supported: false,
            permission_prompt_supported: false,
            accessibility_trusted: false,
            prompt_requested: prompt,
            guidance: "Native Accessibility-backed computer use currently supports macOS only."
                .to_string(),
        }
    }

    pub fn frontmost_app_snapshot(_max_items: usize) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }
}

#[cfg(test)]
mod tests {
    use super::inspect_computer_use;

    #[test]
    fn status_reports_current_platform_without_prompt() {
        let status = inspect_computer_use(false);
        assert_eq!(status.prompt_requested, false);
        assert!(!status.platform.is_empty());
        assert!(!status.guidance.is_empty());
    }
}
