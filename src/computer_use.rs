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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerUseKey {
    pub label: &'static str,
    pub key_code: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerUseScrollDirection {
    pub label: &'static str,
    pub ax_action: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerUseNativeAction {
    pub label: &'static str,
    pub ax_action: &'static str,
}

pub fn inspect_computer_use(prompt: bool) -> ComputerUseStatus {
    platform::inspect_computer_use(prompt)
}

pub fn frontmost_app_snapshot(max_items: usize, max_depth: usize) -> Result<String> {
    platform::frontmost_app_snapshot(max_items, max_depth)
}

pub fn frontmost_app_ref_details(
    reference: &str,
    max_items: usize,
    max_depth: usize,
) -> Result<String> {
    platform::frontmost_app_ref_details(reference, max_items, max_depth)
}

pub fn click_frontmost_app_ref(
    reference: &str,
    max_items: usize,
    max_depth: usize,
) -> Result<String> {
    platform::click_frontmost_app_ref(reference, max_items, max_depth)
}

pub fn focus_frontmost_app_ref(
    reference: &str,
    max_items: usize,
    max_depth: usize,
) -> Result<String> {
    platform::focus_frontmost_app_ref(reference, max_items, max_depth)
}

pub fn set_frontmost_app_ref_text(
    reference: &str,
    text: &str,
    max_items: usize,
    max_depth: usize,
) -> Result<String> {
    platform::set_frontmost_app_ref_text(reference, text, max_items, max_depth)
}

pub fn press_frontmost_app_key(key: &str, max_items: usize, max_depth: usize) -> Result<String> {
    platform::press_frontmost_app_key(normalize_computer_use_key(key)?, max_items, max_depth)
}

pub fn scroll_frontmost_app_ref(
    reference: &str,
    direction: &str,
    steps: usize,
    max_items: usize,
    max_depth: usize,
) -> Result<String> {
    platform::scroll_frontmost_app_ref(
        reference,
        normalize_computer_use_scroll_direction(direction)?,
        steps,
        max_items,
        max_depth,
    )
}

pub fn perform_frontmost_app_ref_action(
    reference: &str,
    native_action: &str,
    max_items: usize,
    max_depth: usize,
) -> Result<String> {
    platform::perform_frontmost_app_ref_action(
        reference,
        normalize_computer_use_native_action(native_action)?,
        max_items,
        max_depth,
    )
}

pub fn parse_ui_ref(reference: &str) -> Result<usize> {
    let trimmed = reference.trim().trim_start_matches('@');
    let number = trimmed
        .strip_prefix('u')
        .ok_or_else(|| anyhow::anyhow!("computer_use ref must look like @u1"))?;
    let index = number
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("computer_use ref must look like @u1"))?;
    if index == 0 {
        anyhow::bail!("computer_use ref indexes start at @u1");
    }
    Ok(index)
}

pub fn normalize_computer_use_key(key: &str) -> Result<ComputerUseKey> {
    let normalized = key.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "enter" | "return" => Ok(ComputerUseKey {
            label: "enter",
            key_code: 36,
        }),
        "escape" | "esc" => Ok(ComputerUseKey {
            label: "escape",
            key_code: 53,
        }),
        "tab" => Ok(ComputerUseKey {
            label: "tab",
            key_code: 48,
        }),
        "space" => Ok(ComputerUseKey {
            label: "space",
            key_code: 49,
        }),
        "backspace" | "delete" => Ok(ComputerUseKey {
            label: "backspace",
            key_code: 51,
        }),
        "forward_delete" | "delete_forward" | "forwarddelete" | "deleteforward" => {
            Ok(ComputerUseKey {
                label: "forward_delete",
                key_code: 117,
            })
        }
        "arrow_up" | "arrowup" | "up" => Ok(ComputerUseKey {
            label: "arrow_up",
            key_code: 126,
        }),
        "arrow_down" | "arrowdown" | "down" => Ok(ComputerUseKey {
            label: "arrow_down",
            key_code: 125,
        }),
        "arrow_left" | "arrowleft" | "left" => Ok(ComputerUseKey {
            label: "arrow_left",
            key_code: 123,
        }),
        "arrow_right" | "arrowright" | "right" => Ok(ComputerUseKey {
            label: "arrow_right",
            key_code: 124,
        }),
        "page_up" | "pageup" => Ok(ComputerUseKey {
            label: "page_up",
            key_code: 116,
        }),
        "page_down" | "pagedown" => Ok(ComputerUseKey {
            label: "page_down",
            key_code: 121,
        }),
        "home" => Ok(ComputerUseKey {
            label: "home",
            key_code: 115,
        }),
        "end" => Ok(ComputerUseKey {
            label: "end",
            key_code: 119,
        }),
        "" => anyhow::bail!("computer_use press_key requires a non-empty `key`"),
        other => anyhow::bail!(
            "unsupported computer_use key `{other}`; allowed keys are enter, escape, tab, space, backspace, forward_delete, arrow_up, arrow_down, arrow_left, arrow_right, page_up, page_down, home, and end"
        ),
    }
}

pub fn normalize_computer_use_scroll_direction(
    direction: &str,
) -> Result<ComputerUseScrollDirection> {
    let normalized = direction
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_");
    match normalized.as_str() {
        "up" | "scroll_up" => Ok(ComputerUseScrollDirection {
            label: "up",
            ax_action: "AXScrollUp",
        }),
        "down" | "scroll_down" => Ok(ComputerUseScrollDirection {
            label: "down",
            ax_action: "AXScrollDown",
        }),
        "left" | "scroll_left" => Ok(ComputerUseScrollDirection {
            label: "left",
            ax_action: "AXScrollLeft",
        }),
        "right" | "scroll_right" => Ok(ComputerUseScrollDirection {
            label: "right",
            ax_action: "AXScrollRight",
        }),
        "" => anyhow::bail!("computer_use scroll requires a non-empty `direction`"),
        other => anyhow::bail!(
            "unsupported computer_use scroll direction `{other}`; allowed directions are up, down, left, and right"
        ),
    }
}

pub fn normalize_computer_use_native_action(action: &str) -> Result<ComputerUseNativeAction> {
    let normalized = action.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let compact = normalized.replace('_', "");
    match compact.as_str() {
        "press" | "axpress" => Ok(ComputerUseNativeAction {
            label: "press",
            ax_action: "AXPress",
        }),
        "showmenu" | "menu" | "axshowmenu" => Ok(ComputerUseNativeAction {
            label: "show_menu",
            ax_action: "AXShowMenu",
        }),
        "confirm" | "axconfirm" => Ok(ComputerUseNativeAction {
            label: "confirm",
            ax_action: "AXConfirm",
        }),
        "cancel" | "axcancel" => Ok(ComputerUseNativeAction {
            label: "cancel",
            ax_action: "AXCancel",
        }),
        "increment" | "axincrement" => Ok(ComputerUseNativeAction {
            label: "increment",
            ax_action: "AXIncrement",
        }),
        "decrement" | "axdecrement" => Ok(ComputerUseNativeAction {
            label: "decrement",
            ax_action: "AXDecrement",
        }),
        "" => anyhow::bail!("computer_use perform_action requires a non-empty `native_action`"),
        other => anyhow::bail!(
            "unsupported computer_use native_action `{other}`; allowed actions are press, show_menu, confirm, cancel, increment, and decrement"
        ),
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{
        ComputerUseKey, ComputerUseNativeAction, ComputerUseScrollDirection, ComputerUseStatus,
        Result, parse_ui_ref,
    };
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

    pub fn set_frontmost_app_ref_text(
        reference: &str,
        text: &str,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use set_text requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let target_index = parse_ui_ref(reference)?;
        let max_items = max_items.clamp(1, 50);
        if target_index > max_items {
            bail!(
                "computer_use ref @u{target_index} is outside max_items={max_items}; use a ref from the latest bounded snapshot or increase max_items up to 50"
            );
        }
        let max_depth = max_depth.clamp(1, 6);
        let script = frontmost_set_text_script(target_index, max_items, max_depth);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        command.arg(text);
        let output = command
            .output()
            .context("failed to run osascript for Accessibility set_text")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility set_text failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let post_snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        Ok(format!(
            "{}\n\npost_set_text_snapshot:\n{}",
            stdout.trim(),
            post_snapshot
        ))
    }

    pub fn press_frontmost_app_key(
        key: ComputerUseKey,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use press_key requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let max_items = max_items.clamp(1, 50);
        let max_depth = max_depth.clamp(1, 6);
        let script = frontmost_press_key_script(key);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility press_key")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility press_key failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let post_snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        Ok(format!(
            "{}\n\npost_key_snapshot:\n{}",
            stdout.trim(),
            post_snapshot
        ))
    }

    pub fn scroll_frontmost_app_ref(
        reference: &str,
        direction: ComputerUseScrollDirection,
        steps: usize,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use scroll requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let target_index = parse_ui_ref(reference)?;
        let max_items = max_items.clamp(1, 50);
        if target_index > max_items {
            bail!(
                "computer_use ref @u{target_index} is outside max_items={max_items}; use a ref from the latest bounded snapshot or increase max_items up to 50"
            );
        }
        let max_depth = max_depth.clamp(1, 6);
        let steps = steps.clamp(1, 10);
        let script = frontmost_scroll_script(target_index, direction, steps, max_items, max_depth);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility scroll")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility scroll failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let post_snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        Ok(format!(
            "{}\n\npost_scroll_snapshot:\n{}",
            stdout.trim(),
            post_snapshot
        ))
    }

    pub fn perform_frontmost_app_ref_action(
        reference: &str,
        native_action: ComputerUseNativeAction,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use perform_action requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let target_index = parse_ui_ref(reference)?;
        let max_items = max_items.clamp(1, 50);
        if target_index > max_items {
            bail!(
                "computer_use ref @u{target_index} is outside max_items={max_items}; use a ref from the latest bounded snapshot or increase max_items up to 50"
            );
        }
        let max_depth = max_depth.clamp(1, 6);
        let script =
            frontmost_perform_action_script(target_index, native_action, max_items, max_depth);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility perform_action")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility perform_action failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let post_snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        Ok(format!(
            "{}\n\npost_action_snapshot:\n{}",
            stdout.trim(),
            post_snapshot
        ))
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

    pub fn frontmost_app_ref_details(
        reference: &str,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use inspect_ref requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let target_index = parse_ui_ref(reference)?;
        let max_items = max_items.clamp(1, 50);
        if target_index > max_items {
            bail!(
                "computer_use ref @u{target_index} is outside max_items={max_items}; use a ref from the latest bounded snapshot or increase max_items up to 50"
            );
        }
        let max_depth = max_depth.clamp(1, 6);
        let script = frontmost_ref_details_script(target_index, max_items, max_depth);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility inspect_ref")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility inspect_ref failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    }

    pub fn click_frontmost_app_ref(
        reference: &str,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use click requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let target_index = parse_ui_ref(reference)?;
        let max_items = max_items.clamp(1, 50);
        if target_index > max_items {
            bail!(
                "computer_use ref @u{target_index} is outside max_items={max_items}; use a ref from the latest bounded snapshot or increase max_items up to 50"
            );
        }
        let max_depth = max_depth.clamp(1, 6);
        let script = frontmost_click_script(target_index, max_items, max_depth);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility click")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility click failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let post_snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        Ok(format!(
            "{}\n\npost_click_snapshot:\n{}",
            stdout.trim(),
            post_snapshot
        ))
    }

    pub fn focus_frontmost_app_ref(
        reference: &str,
        max_items: usize,
        max_depth: usize,
    ) -> Result<String> {
        if !accessibility_trusted(false) {
            bail!(
                "computer_use focus requires macOS Accessibility permission. Run action=request_permission, then enable Crab or the launching terminal in System Settings > Privacy & Security > Accessibility."
            );
        }

        let target_index = parse_ui_ref(reference)?;
        let max_items = max_items.clamp(1, 50);
        if target_index > max_items {
            bail!(
                "computer_use ref @u{target_index} is outside max_items={max_items}; use a ref from the latest bounded snapshot or increase max_items up to 50"
            );
        }
        let max_depth = max_depth.clamp(1, 6);
        let script = frontmost_focus_script(target_index, max_items, max_depth);
        let mut command = Command::new("osascript");
        for line in &script {
            command.arg("-e").arg(line);
        }
        let output = command
            .output()
            .context("failed to run osascript for Accessibility focus")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Accessibility focus failed: {}",
                stderr.trim().trim_end_matches('.')
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let post_snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        Ok(format!(
            "{}\n\npost_focus_snapshot:\n{}",
            stdout.trim(),
            post_snapshot
        ))
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
            "try",
            "set enabledText to enabled of elementRef as text",
            "on error",
            "set enabledText to \"\"",
            "end try",
            "try",
            "set focusedText to focused of elementRef as text",
            "on error",
            "set focusedText to \"\"",
            "end try",
            "try",
            "set selectedText to selected of elementRef as text",
            "on error",
            "set selectedText to \"\"",
            "end try",
            "set itemIndex to itemIndex + 1",
            "set lineText to \"- @u\" & itemIndex & \" role=\" & quoted form of (my cleanText(roleText))",
            "if nameText is not \"\" then set lineText to lineText & \" name=\" & quoted form of (my cleanText(nameText))",
            "if valueText is not \"\" then set lineText to lineText & \" value=\" & quoted form of (my cleanText(valueText))",
            "set lineText to lineText & boundsText",
            "if enabledText is \"false\" then set lineText to lineText & \" enabled=false\"",
            "if focusedText is \"true\" then set lineText to lineText & \" focused=true\"",
            "if selectedText is \"true\" then set lineText to lineText & \" selected=true\"",
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

    fn frontmost_focus_script(
        target_index: usize,
        max_items: usize,
        max_depth: usize,
    ) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth, targetIndex, didFocus",
            &format!("set targetIndex to {target_index}"),
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            "set itemIndex to 0",
            "set didFocus to false",
            "on visitElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth, targetIndex, didFocus",
            "if itemIndex is greater than or equal to maxItems then return false",
            "tell application \"System Events\"",
            "set itemIndex to itemIndex + 1",
            "if itemIndex is targetIndex then",
            "try",
            "set focused of elementRef to true",
            "delay 0.15",
            "set didFocus to true",
            "return true",
            "on error errMsg",
            "error \"failed to focus @u\" & targetIndex & \": \" & errMsg",
            "end try",
            "end if",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(childElement, depth + 1) then return true",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return false",
            "end visitElement",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(windowRef, 0) then exit repeat",
            "end repeat",
            "if didFocus is false then error \"UI ref @u\" & targetIndex & \" was not found in the current Accessibility snapshot\"",
            "return \"focused_ref: @u\" & targetIndex & linefeed & \"frontmost_app_before_focus: \" & appName",
            "end tell",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn frontmost_ref_details_script(
        target_index: usize,
        max_items: usize,
        max_depth: usize,
    ) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth, targetIndex, foundOutput",
            &format!("set targetIndex to {target_index}"),
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            "set itemIndex to 0",
            "set foundOutput to \"\"",
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
            "on describeTarget(elementRef)",
            "global itemIndex, targetIndex",
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
            "try",
            "set enabledText to enabled of elementRef as text",
            "on error",
            "set enabledText to \"\"",
            "end try",
            "try",
            "set focusedText to focused of elementRef as text",
            "on error",
            "set focusedText to \"\"",
            "end try",
            "try",
            "set selectedText to selected of elementRef as text",
            "on error",
            "set selectedText to \"\"",
            "end try",
            "set lineText to \"- @u\" & targetIndex & \" role=\" & quoted form of (my cleanText(roleText))",
            "if nameText is not \"\" then set lineText to lineText & \" name=\" & quoted form of (my cleanText(nameText))",
            "if valueText is not \"\" then set lineText to lineText & \" value=\" & quoted form of (my cleanText(valueText))",
            "set lineText to lineText & boundsText",
            "if enabledText is \"false\" then set lineText to lineText & \" enabled=false\"",
            "if focusedText is \"true\" then set lineText to lineText & \" focused=true\"",
            "if selectedText is \"true\" then set lineText to lineText & \" selected=true\"",
            "set actionNames to \"\"",
            "try",
            "set actionRefs to actions of elementRef",
            "repeat with actionRef in actionRefs",
            "try",
            "set actionName to name of actionRef as text",
            "if actionNames is \"\" then",
            "set actionNames to actionName",
            "else",
            "set actionNames to actionNames & \", \" & actionName",
            "end if",
            "end try",
            "end repeat",
            "end try",
            "if actionNames is \"\" then set actionNames to \"(none reported)\"",
            "return \"ref: @u\" & targetIndex & linefeed & \"ref_line: \" & lineText & linefeed & \"available_actions: \" & actionNames",
            "end tell",
            "end describeTarget",
            "on visitElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth, targetIndex, foundOutput",
            "if itemIndex is greater than or equal to maxItems then return false",
            "tell application \"System Events\"",
            "set itemIndex to itemIndex + 1",
            "if itemIndex is targetIndex then",
            "set foundOutput to my describeTarget(elementRef)",
            "return true",
            "end if",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(childElement, depth + 1) then return true",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return false",
            "end visitElement",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "set appPid to unix id of frontApp",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(windowRef, 0) then exit repeat",
            "end repeat",
            "if foundOutput is \"\" then error \"UI ref @u\" & targetIndex & \" was not found in the current Accessibility snapshot\"",
            "return \"frontmost_app: \" & appName & linefeed & \"pid: \" & appPid & linefeed & foundOutput",
            "end tell",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn frontmost_click_script(
        target_index: usize,
        max_items: usize,
        max_depth: usize,
    ) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth, targetIndex, didClick",
            &format!("set targetIndex to {target_index}"),
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            "set itemIndex to 0",
            "set didClick to false",
            "on visitElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth, targetIndex, didClick",
            "if itemIndex is greater than or equal to maxItems then return false",
            "tell application \"System Events\"",
            "set itemIndex to itemIndex + 1",
            "if itemIndex is targetIndex then",
            "try",
            "click elementRef",
            "delay 0.15",
            "set didClick to true",
            "return true",
            "on error errMsg",
            "error \"failed to click @u\" & targetIndex & \": \" & errMsg",
            "end try",
            "end if",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(childElement, depth + 1) then return true",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return false",
            "end visitElement",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(windowRef, 0) then exit repeat",
            "end repeat",
            "if didClick is false then error \"UI ref @u\" & targetIndex & \" was not found in the current Accessibility snapshot\"",
            "return \"clicked_ref: @u\" & targetIndex & linefeed & \"frontmost_app_before_click: \" & appName",
            "end tell",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn frontmost_set_text_script(
        target_index: usize,
        max_items: usize,
        max_depth: usize,
    ) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth, targetIndex, didSetText, replacementText",
            "on visitElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth, targetIndex, didSetText, replacementText",
            "if itemIndex is greater than or equal to maxItems then return false",
            "tell application \"System Events\"",
            "set itemIndex to itemIndex + 1",
            "if itemIndex is targetIndex then",
            "try",
            "set value of elementRef to replacementText",
            "delay 0.15",
            "set didSetText to true",
            "return true",
            "on error errMsg",
            "error \"failed to set text for @u\" & targetIndex & \": \" & errMsg",
            "end try",
            "end if",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(childElement, depth + 1) then return true",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return false",
            "end visitElement",
            "on run argv",
            "global itemIndex, maxItems, maxDepth, targetIndex, didSetText, replacementText",
            &format!("set targetIndex to {target_index}"),
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            "if (count of argv) is less than 1 then error \"missing replacement text argument\"",
            "set replacementText to item 1 of argv",
            "set itemIndex to 0",
            "set didSetText to false",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(windowRef, 0) then exit repeat",
            "end repeat",
            "if didSetText is false then error \"UI ref @u\" & targetIndex & \" was not found in the current Accessibility snapshot\"",
            "set replacementLength to length of replacementText",
            "return \"set_text_ref: @u\" & targetIndex & linefeed & \"frontmost_app_before_set_text: \" & appName & linefeed & \"text_chars: \" & replacementLength",
            "end tell",
            "end run",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn frontmost_scroll_script(
        target_index: usize,
        direction: ComputerUseScrollDirection,
        steps: usize,
        max_items: usize,
        max_depth: usize,
    ) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth, targetIndex, didScroll, scrollAction, scrollSteps",
            &format!("set targetIndex to {target_index}"),
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            &format!("set scrollAction to \"{}\"", direction.ax_action),
            &format!("set scrollSteps to {steps}"),
            "set itemIndex to 0",
            "set didScroll to false",
            "on visitElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth, targetIndex, didScroll, scrollAction, scrollSteps",
            "if itemIndex is greater than or equal to maxItems then return false",
            "tell application \"System Events\"",
            "set itemIndex to itemIndex + 1",
            "if itemIndex is targetIndex then",
            "try",
            "repeat scrollSteps times",
            "perform action scrollAction of elementRef",
            "delay 0.05",
            "end repeat",
            "delay 0.15",
            "set didScroll to true",
            "return true",
            "on error errMsg",
            "error \"failed to scroll @u\" & targetIndex & \": \" & errMsg",
            "end try",
            "end if",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(childElement, depth + 1) then return true",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return false",
            "end visitElement",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(windowRef, 0) then exit repeat",
            "end repeat",
            "if didScroll is false then error \"UI ref @u\" & targetIndex & \" was not found in the current Accessibility snapshot\"",
            &format!(
                "return \"scrolled_ref: @u\" & targetIndex & linefeed & \"scroll_direction: {}\" & linefeed & \"scroll_steps: \" & scrollSteps & linefeed & \"frontmost_app_before_scroll: \" & appName",
                direction.label
            ),
            "end tell",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn frontmost_perform_action_script(
        target_index: usize,
        native_action: ComputerUseNativeAction,
        max_items: usize,
        max_depth: usize,
    ) -> Vec<String> {
        [
            "global itemIndex, maxItems, maxDepth, targetIndex, didPerformAction, nativeAction",
            &format!("set targetIndex to {target_index}"),
            &format!("set maxItems to {max_items}"),
            &format!("set maxDepth to {max_depth}"),
            &format!("set nativeAction to \"{}\"", native_action.ax_action),
            "set itemIndex to 0",
            "set didPerformAction to false",
            "on visitElement(elementRef, depth)",
            "global itemIndex, maxItems, maxDepth, targetIndex, didPerformAction, nativeAction",
            "if itemIndex is greater than or equal to maxItems then return false",
            "tell application \"System Events\"",
            "set itemIndex to itemIndex + 1",
            "if itemIndex is targetIndex then",
            "try",
            "perform action nativeAction of elementRef",
            "delay 0.15",
            "set didPerformAction to true",
            "return true",
            "on error errMsg",
            "error \"failed to perform \" & nativeAction & \" for @u\" & targetIndex & \": \" & errMsg",
            "end try",
            "end if",
            "if depth is less than maxDepth then",
            "try",
            "set childElements to UI elements of elementRef",
            "repeat with childElement in childElements",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(childElement, depth + 1) then return true",
            "end repeat",
            "end try",
            "end if",
            "end tell",
            "return false",
            "end visitElement",
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            "repeat with windowRef in windows of frontApp",
            "if itemIndex is greater than or equal to maxItems then exit repeat",
            "if my visitElement(windowRef, 0) then exit repeat",
            "end repeat",
            "if didPerformAction is false then error \"UI ref @u\" & targetIndex & \" was not found in the current Accessibility snapshot\"",
            &format!(
                "return \"performed_action_ref: @u\" & targetIndex & linefeed & \"native_action: {}\" & linefeed & \"ax_action: {}\" & linefeed & \"frontmost_app_before_action: \" & appName",
                native_action.label, native_action.ax_action
            ),
            "end tell",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn frontmost_press_key_script(key: ComputerUseKey) -> Vec<String> {
        [
            "tell application \"System Events\"",
            "set frontApp to first application process whose frontmost is true",
            "set appName to name of frontApp",
            &format!("key code {}", key.key_code),
            "delay 0.15",
            &format!(
                "return \"pressed_key: {}\" & linefeed & \"frontmost_app_before_key: \" & appName",
                key.label
            ),
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
        use super::{
            frontmost_click_script, frontmost_focus_script, frontmost_perform_action_script,
            frontmost_press_key_script, frontmost_ref_details_script, frontmost_scroll_script,
            frontmost_set_text_script, frontmost_snapshot_script,
        };
        use crate::computer_use::{
            normalize_computer_use_key, normalize_computer_use_native_action,
            normalize_computer_use_scroll_direction,
        };
        use std::process::Command;

        #[test]
        fn snapshot_script_includes_ui_tree_and_refs() {
            let script = frontmost_snapshot_script(8, 2).join("\n");

            assert!(script.contains("set maxItems to 8"));
            assert!(script.contains("set maxDepth to 2"));
            assert!(script.contains("ui_tree:"));
            assert!(script.contains("@u"));
            assert!(script.contains("role description"));
            assert!(script.contains("enabled=false"));
            assert!(script.contains("focused=true"));
            assert!(script.contains("selected=true"));
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

        #[test]
        fn click_script_compiles() {
            let script = frontmost_click_script(2, 8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-click.scpt");
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

        #[test]
        fn focus_script_compiles() {
            let script = frontmost_focus_script(2, 8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-focus.scpt");
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

        #[test]
        fn ref_details_script_compiles() {
            let script = frontmost_ref_details_script(2, 8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-ref-details.scpt");
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

        #[test]
        fn set_text_script_compiles() {
            let script = frontmost_set_text_script(2, 8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-set-text.scpt");
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

        #[test]
        fn press_key_script_compiles() {
            let key = normalize_computer_use_key("enter").expect("key");
            let script = frontmost_press_key_script(key);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-press-key.scpt");
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

        #[test]
        fn scroll_script_compiles() {
            let direction = normalize_computer_use_scroll_direction("down").expect("direction");
            let script = frontmost_scroll_script(2, direction, 2, 8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-scroll.scpt");
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

        #[test]
        fn perform_action_script_compiles() {
            let native_action = normalize_computer_use_native_action("press").expect("action");
            let script = frontmost_perform_action_script(2, native_action, 8, 2);
            let tmp = tempfile::tempdir().expect("tempdir");
            let output_path = tmp.path().join("computer-use-perform-action.scpt");
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
    use super::{
        ComputerUseKey, ComputerUseNativeAction, ComputerUseScrollDirection, ComputerUseStatus,
        Result,
    };
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

    pub fn frontmost_app_ref_details(
        _reference: &str,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }

    pub fn click_frontmost_app_ref(
        _reference: &str,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }

    pub fn focus_frontmost_app_ref(
        _reference: &str,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }

    pub fn set_frontmost_app_ref_text(
        _reference: &str,
        _text: &str,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }

    pub fn press_frontmost_app_key(
        _key: ComputerUseKey,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }

    pub fn scroll_frontmost_app_ref(
        _reference: &str,
        _direction: ComputerUseScrollDirection,
        _steps: usize,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }

    pub fn perform_frontmost_app_ref_action(
        _reference: &str,
        _native_action: ComputerUseNativeAction,
        _max_items: usize,
        _max_depth: usize,
    ) -> Result<String> {
        bail!("native Accessibility-backed computer use currently supports macOS only")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        inspect_computer_use, normalize_computer_use_key, normalize_computer_use_native_action,
        normalize_computer_use_scroll_direction, parse_ui_ref,
    };

    #[test]
    fn status_reports_current_platform_without_prompt() {
        let status = inspect_computer_use(false);
        assert_eq!(status.prompt_requested, false);
        assert!(!status.platform.is_empty());
        assert!(!status.guidance.is_empty());
    }

    #[test]
    fn parses_ui_refs() {
        assert_eq!(parse_ui_ref("@u12").expect("ref"), 12);
        assert_eq!(parse_ui_ref("u3").expect("ref"), 3);
        assert!(parse_ui_ref("@e1").is_err());
        assert!(parse_ui_ref("@u0").is_err());
    }

    #[test]
    fn normalizes_allowed_computer_use_keys() {
        assert_eq!(
            normalize_computer_use_key("Return").expect("key").label,
            "enter"
        );
        assert_eq!(
            normalize_computer_use_key("arrow-left")
                .expect("key")
                .key_code,
            123
        );
        assert_eq!(
            normalize_computer_use_key("arrowLeft").expect("key").label,
            "arrow_left"
        );
        assert_eq!(
            normalize_computer_use_key("Page Down").expect("key").label,
            "page_down"
        );
    }

    #[test]
    fn rejects_unsupported_computer_use_keys() {
        let error = normalize_computer_use_key("a").expect_err("unsupported key");
        assert!(format!("{error:#}").contains("unsupported computer_use key"));
    }

    #[test]
    fn normalizes_computer_use_scroll_directions() {
        assert_eq!(
            normalize_computer_use_scroll_direction("scroll-down")
                .expect("direction")
                .ax_action,
            "AXScrollDown"
        );
        assert_eq!(
            normalize_computer_use_scroll_direction("LEFT")
                .expect("direction")
                .label,
            "left"
        );
    }

    #[test]
    fn rejects_unsupported_computer_use_scroll_directions() {
        let error =
            normalize_computer_use_scroll_direction("diagonal").expect_err("unsupported direction");
        assert!(format!("{error:#}").contains("unsupported computer_use scroll direction"));
    }

    #[test]
    fn normalizes_computer_use_native_actions() {
        assert_eq!(
            normalize_computer_use_native_action("AXPress")
                .expect("action")
                .label,
            "press"
        );
        assert_eq!(
            normalize_computer_use_native_action("show-menu")
                .expect("action")
                .ax_action,
            "AXShowMenu"
        );
        assert_eq!(
            normalize_computer_use_native_action("decrement")
                .expect("action")
                .ax_action,
            "AXDecrement"
        );
    }

    #[test]
    fn rejects_unsupported_computer_use_native_actions() {
        let error =
            normalize_computer_use_native_action("raise").expect_err("unsupported native action");
        assert!(format!("{error:#}").contains("unsupported computer_use native_action"));
    }
}
