use crate::theme;
use dear_imgui_rs::{Condition, Ui, WindowFlags, sys};

pub const PARITY_CASE: &str = "modal-linux-input-setup-user-reference";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinuxInputSetupStatus {
    pub required: bool,
    pub summary: String,
    pub details: String,
    pub installer_path: String,
    pub manual_command: String,
    pub docs_path: String,
    pub action_message: String,
    pub busy: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxInputSetupAction {
    InstallGraphical,
    OpenTerminalInstaller,
    CopyManualCommand,
    OpenSetupDocs,
    Retry,
}

#[cfg(target_os = "linux")]
#[must_use]
pub fn parity_reference_status() -> LinuxInputSetupStatus {
    LinuxInputSetupStatus {
        required: true,
        summary: "Parity permission summary: Linux input devices are not accessible.".to_owned(),
        details: "/dev/input/event0: Permission denied\n/dev/uinput: Permission denied".to_owned(),
        installer_path: "/tmp/smu-parity/install_linux_permissions.sh".to_owned(),
        manual_command: "sudo /tmp/smu-parity/install_linux_permissions.sh".to_owned(),
        docs_path: "/tmp/smu-parity/LINUX_SETUP.md".to_owned(),
        action_message: String::new(),
        busy: false,
    }
}

fn set_next_modal_layout(ui: &Ui) {
    let viewport = ui.main_viewport();
    let pos = viewport.pos();
    let size = viewport.size();
    let center = [size[0].mul_add(0.5, pos[0]), size[1].mul_add(0.5, pos[1])];
    // SAFETY: these calls configure the immediately following modal Begin while `ui` owns the
    // active Dear ImGui context for this frame.
    unsafe {
        sys::igSetNextWindowPos(
            sys::ImVec2_c { x: center[0], y: center[1] },
            Condition::Appearing as i32,
            sys::ImVec2_c { x: 0.5, y: 0.5 },
        );
        sys::igSetNextWindowSize(sys::ImVec2_c { x: 620.0, y: 0.0 }, Condition::Appearing as i32);
    }
}

pub fn render(
    ui: &Ui,
    status: &LinuxInputSetupStatus,
    _theme: &theme::ThemeDefinition,
) -> Option<LinuxInputSetupAction> {
    if !status.required {
        return None;
    }

    let _ = ui.open_popup("Linux Input Setup Required");
    set_next_modal_layout(ui);
    let _popup = ui
        .begin_modal_popup_config("Linux Input Setup Required")
        .flags(WindowFlags::ALWAYS_AUTO_RESIZE)
        .begin()?;

    ui.text_wrapped("Native Linux input requires access to /dev/input/event* and /dev/uinput.");
    ui.text_wrapped(
        "SMU can install a one-time udev rule and add your user to the smu-input group. The graphical installer uses your desktop polkit agent. If no authentication window appears, use the terminal installer or copy the manual sudo command.",
    );
    crate::parity::separator(ui);
    ui.text_wrapped(&status.summary);
    {
        let _wrap = ui.push_text_wrap_pos(ui.cursor_pos()[0] + 580.0);
        ui.text(&status.details);
    }
    crate::parity::separator(ui);

    let disabled = status.busy.then(|| ui.begin_disabled());
    let install = ui.button("Install permissions");
    ui.same_line();
    let terminal = ui.button("Open terminal installer");
    drop(disabled);

    let copy = ui.button("Copy manual sudo command");
    ui.same_line();
    let docs = ui.button("Open setup docs");
    let retry = ui.button("Retry permission check");

    if status.busy {
        ui.same_line();
        ui.text_disabled("Working...");
    }
    if !status.action_message.is_empty() {
        crate::parity::separator(ui);
        let _wrap = ui.push_text_wrap_pos(ui.cursor_pos()[0] + 580.0);
        ui.text(&status.action_message);
    }

    if install {
        return Some(LinuxInputSetupAction::InstallGraphical);
    }
    if terminal {
        return Some(LinuxInputSetupAction::OpenTerminalInstaller);
    }
    if copy {
        return Some(LinuxInputSetupAction::CopyManualCommand);
    }
    if docs {
        return Some(LinuxInputSetupAction::OpenSetupDocs);
    }
    retry.then_some(LinuxInputSetupAction::Retry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_does_not_require_setup() {
        let status = LinuxInputSetupStatus::default();
        assert!(!status.required);
        assert!(!status.busy);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parity_reference_status_is_deterministic_and_required() {
        let status = parity_reference_status();
        assert!(status.required);
        assert!(!status.busy);
        assert_eq!(
            status.summary,
            "Parity permission summary: Linux input devices are not accessible."
        );
        assert_eq!(
            status.details,
            "/dev/input/event0: Permission denied\n/dev/uinput: Permission denied"
        );
        assert!(status.action_message.is_empty());
    }
}
