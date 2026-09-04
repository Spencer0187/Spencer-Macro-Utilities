use crate::theme;
use dear_imgui_rs::Ui;

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

pub fn render(
    ui: &Ui,
    status: &LinuxInputSetupStatus,
    theme: &theme::ThemeDefinition,
) -> Option<LinuxInputSetupAction> {
    if !status.required {
        return None;
    }

    let _ = ui.open_popup("Linux Input Setup Required");
    let _popup = ui.begin_modal_popup("Linux Input Setup Required")?;

    ui.text_wrapped("Native Linux input requires access to /dev/input/event* and /dev/uinput.");
    ui.text_wrapped(
        "SMU can install a one-time udev rule and add your user to the smu-input group. The graphical installer uses your desktop polkit agent. If no authentication window appears, use the terminal installer or copy the manual sudo command.",
    );
    crate::parity::separator(ui);
    ui.text_wrapped(&status.summary);
    ui.text_wrapped(&status.details);
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
        ui.text_colored(theme.text_primary, &status.action_message);
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
}
