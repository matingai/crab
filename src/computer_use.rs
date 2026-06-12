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

pub fn frontmost_app_snapshot(max_items: usize, max_depth: usize) -> Result<String> {
    platform::frontmost_app_snapshot(max_items, max_depth)
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

    pub fn frontmost_app_snapshot(max_items: usize, max_depth: usize) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use snapshot requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let max_items = max_items.clamp(1, 50);
        let max_depth = max_depth.clamp(1, 6);
        let script = frontmost_snapshot_script(max_items, max_depth);

        let mut command = Command::new("osascript");
        for line in &script {
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

    fn frontmost_snapshot_script(max_items: usize, max_depth: usize) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth",
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            "set itemIndex to 0",
            "on cleanText(valueText)",
            "try",
            "set textValue to valueText as text",
            "on error",
            "return \"\"",
            "end try",
            "set oldDelimiters to AppleScript's text item delimiters",
            "set AppleScript's text item delimiters to linefeed",
            "set textItems to text items of textValue",
            "set AppleScript's text item delimiters to \" \"",
            "set textValue to textItems as text",
            "set AppleScript's text item delimiters to return",
            "set textItems to text items of textValue",
            "set AppleScript's text item delimiters to \" \"",
            "set textValue to textItems as text",
            "set AppleScript's text item delimiters to oldDelimiters",
            "if length of textValue is greater than 120 then set textValue to text 1 thru 117 of textValue & \"...\"",
            "return textValue",
            "end cleanText",
            "on indentFor(depth)",
            "set indentText to \"\"",
            "repeat depth times",
            "set indentText to indentText & \"  \"",
            "end repeat",
            "return indentText",
            "end indentFor",
            "on describeElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth",
            "if itemIndex is greater than or equal to maxItems then return \"\"",
            "tell application \"System Events\"",
            "try",
            "set roleText to role description of elementRef as text",
            "on error",
            "try",
            "set roleText to role of elementRef as text",
            "on error",
            "set roleText to \"unknown\"",
            "end try",
            "end try",
            "try",
            "set nameText to name of elementRef as text",
            "on error",
            "set nameText to \"\"",
            "end try",
            "try",
            "set valueText to value of elementRef as text",
            "on error",
            "set valueText to \"\"",
            "end try",
            "try",
            "set {x, y} to position of elementRef",
            "set {wide, high} to size of elementRef",
            "set boundsText to \" bounds=(\" & x & \",\" & y & \",\" & wide & \"x\" & high & \")\"",
            "on error",
            "set boundsText to \"\"",
            "end try",
            "set itemIndex to itemIndex + 1",
            "set lineText to \"- @u\" & itemIndex & \" role=\" & quoted form of (my cleanText(roleText))",
            "if nameText is not \"\" then set lineText to lineText & \" name=\" & quoted form of (my cleanText(nameText))",
            "if valueText is not \"\" then set lineText to lineText & \" value=\" & quoted form of (my cleanText(valueText))",
            "set lineText to lineText & boundsText",
            "set localOutput to linefeed & my indentFor(depth) & lineText",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "set localOutput to localOutput & my describeElement(childElement, depth + 1)",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return localOutput",
            "end describeElement",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "set appPid to unix id of frontApp",
            "set output to \"frontmost_app: \" & appName & linefeed & \"pid: \" & appPid",
            "set output to output & linefeed & \"ui_tree:\"",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "set output to output & my describeElement(windowRef, 0)",
            "end repeat",
            "if itemIndex is 0 then set output to output & linefeed & \"(no accessibility elements returned)\"",
            "return output",
            "end tell",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
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

    #[cfg(test)]
    mod tests {
        use super::frontmost_snapshot_script;
        use std::process::Command;

        #[test]
        fn snapshot_script_includes_ui_tree_and_refs() {
            let script = frontmost_snapshot_script(8, 2).join("\n");

            assert!(script.contains("set maxItems to 8"));
            assert!(script.contains("set maxDepth to 2"));
            assert!(script.contains("ui_tree:"));
            assert!(script.contains("@u"));
            assert!(script.contains("role description"));
        }

        #[test]
        fn snapshot_script_compiles() {
            let script = frontmost_snapshot_script(8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-snapshot.scpt");
            let mut command = Command::new("osacompile");
            command.arg("-o").arg(&output_path);
            for line in script {
                command.arg("-e").arg(line);
            }

            let output = command.output().expect("run osacompile");
            assert!(
                output.status.success(),
                "osacompile failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
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

    pub fn frontmost_app_snapshot(_max_items: usize, _max_depth: usize) -> Result<String> {
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
