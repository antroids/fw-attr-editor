// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::application::controls::Control;
use crate::sysfs_firmware_attributes::{
    autodetect_root, Attribute, AttributeError, AttributeParser, Mechanism,
};
use chrono::{DateTime, Local};
use eframe::glow::Context;
use egui::{Key, RichText};
use log::{error, info};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod controls;

pub enum Application<T: AttributeParser> {
    SelectRoot {
        roots: Vec<PathBuf>,
        status: Status,
    },
    BiosAdminAuthentication {
        root: PathBuf,
        authentication: T::Auth,
        password: String,
        status: Status,
    },
    BiosAttributes {
        root: PathBuf,
        access_mode: AccessMode<T>,
        controls: Vec<Control<T>>,
        status: Status,
    },
}

impl<T: AttributeParser> Application<T> {
    fn status(&self) -> Status {
        match self {
            Application::BiosAdminAuthentication { status, .. } => status.clone(),
            Application::BiosAttributes { status, .. } => status.clone(),
            Application::SelectRoot { status, .. } => status.clone(),
        }
    }

    fn check_pending_reboot(root: &Path, status: &Status) {
        status.inner.lock().unwrap().reboot_required = matches!(T::pending_reboot(root), Ok(true));
    }

    pub fn autodetect_root() -> Self {
        let roots = autodetect_root();
        Self::select_root(roots)
    }

    pub fn select_root(roots: Vec<PathBuf>) -> Self {
        Self::SelectRoot {
            roots,
            status: Status::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Status {
    inner: Arc<Mutex<StatusInner>>,
}

impl Status {
    fn inner(&self) -> StatusInner {
        self.inner.lock().unwrap().clone()
    }

    fn handle_result<R>(&self, result: Result<R, impl Error>) -> Option<R> {
        let mut inner = self.inner.lock().unwrap();
        inner.changed = Local::now();
        match result {
            Ok(result) => {
                if matches!(inner.message, StatusMessage::Error(_)) {
                    inner.message = StatusMessage::Ok;
                }
                Some(result)
            }
            Err(err) => {
                error!("{:?}", err);
                inner.message = StatusMessage::Error(err.to_string());
                None
            }
        }
    }

    fn handle_result_with_message<R>(
        &self,
        result: Result<R, impl Error>,
        message: &str,
    ) -> Option<R> {
        let mut inner = self.inner.lock().unwrap();
        inner.changed = Local::now();
        match result {
            Ok(result) => {
                info!("Success: {:?}", message);
                inner.message = StatusMessage::Message(message.to_string());
                Some(result)
            }
            Err(err) => {
                error!("{:?}", err);
                inner.message = StatusMessage::Error(err.to_string());
                None
            }
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StatusInner {
                changed: Local::now(),
                message: StatusMessage::Ok,
                reboot_required: false,
            })),
        }
    }
}

#[derive(Debug, Clone)]
struct StatusInner {
    changed: DateTime<Local>,
    message: StatusMessage,
    reboot_required: bool,
}

#[derive(Clone, Debug)]
enum StatusMessage {
    Ok,
    Message(String),
    Error(String),
}

pub enum AccessMode<T: AttributeParser> {
    ReadOnly,
    ReadWrite,
    ReadWriteAuthenticated(T::Auth),
}

impl<T: AttributeParser> AccessMode<T> {
    pub fn write_access(&self) -> bool {
        match self {
            AccessMode::ReadOnly => false,
            _ => true,
        }
    }
}

impl eframe::App for Application<Attribute> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("Header").show(ctx, |ui| {
            self.header_bar(ui);
        });
        egui::TopBottomPanel::bottom("Status").show(ctx, |ui| {
            self.status_bar(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self {
            Application::BiosAdminAuthentication { .. } => {
                self.bios_admin_authentication_ui(ui);
            }
            Application::BiosAttributes { .. } => {
                self.attributes_edit_form(ui);
            }
            Application::SelectRoot { roots, status } => {
                if roots.len() == 1 {
                    let root = roots.first().unwrap();
                    if let Some(state) = status.handle_result_with_message(
                        Self::bios_admin_authentication(root, status),
                        &format!("The Only Root {:?} was selected automatically", root),
                    ) {
                        *self = state;
                        ctx.request_repaint();
                    }
                } else {
                    self.select_root_ui(ui);
                }
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&Context>) {
        if let Self::BiosAttributes {
            access_mode: AccessMode::ReadWriteAuthenticated(auth),
            ..
        } = self
        {
            // Logout
            let _ = auth.authenticate_with_password("");
        }
    }
}

impl Application<Attribute> {
    pub fn bios_attributes(
        path: &Path,
        access_mode: AccessMode<Attribute>,
        status: &Status,
    ) -> Result<Self, AttributeError> {
        let attributes_names = Attribute::attributes_names(path).unwrap();
        let controls: Vec<Control<Attribute>> = attributes_names
            .iter()
            .filter_map(|name| Attribute::attribute(path, name).ok())
            .map(|attribute| Control::new(attribute, status))
            .collect();
        Self::check_pending_reboot(path, &status);
        Ok(Self::BiosAttributes {
            root: path.to_path_buf(),
            access_mode,
            controls,
            status: status.clone(),
        })
    }

    pub fn bios_admin_authentication(path: &Path, status: &Status) -> Result<Self, AttributeError> {
        let authentication_names = Attribute::authentications_names(path)?;
        for name in authentication_names {
            let authentication = Attribute::authentication(path, &name)?;
            if authentication.is_enabled && matches!(authentication.mechanism, Mechanism::Password)
            {
                return Ok(Self::BiosAdminAuthentication {
                    root: path.to_path_buf(),
                    authentication,
                    password: String::new(),
                    status: status.clone(),
                });
            }
        }
        Self::bios_attributes(path, AccessMode::ReadWrite, status)
    }

    fn bios_admin_authentication_ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                [200.0, 10.0].into(),
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    if let Application::BiosAdminAuthentication {
                        root,
                        authentication,
                        password,
                        status,
                    } = self
                    {
                        ui.label(format!("Login: {}", &authentication.login));
                        ui.label(format!("Role: {:?}", &authentication.role));
                        ui.label("BIOS Administrator Password: ");
                        let input_response =
                            ui.add(egui::TextEdit::singleline(password).password(true));
                        if ui.memory(|m| m.focus().is_none()) {
                            input_response.request_focus();
                        }
                        if ui.button("Login").clicked()
                            || (input_response.has_focus()
                                && ui.input(|i| i.key_pressed(Key::Enter)))
                        {
                            if status
                                .handle_result(authentication.authenticate_with_password(&password))
                                .is_some()
                            {
                                let access_mode =
                                    AccessMode::ReadWriteAuthenticated(authentication.clone());
                                if let Some(state) = status.handle_result_with_message(
                                    Self::bios_attributes(root, access_mode, status),
                                    "Logged in",
                                ) {
                                    *self = state;
                                }
                            }
                        } else if ui.button("Proceed without Authentication").clicked() {
                            let access_mode = AccessMode::ReadOnly;
                            if let Some(state) = status.handle_result_with_message(
                                Self::bios_attributes(root, access_mode, status),
                                "Read only mode",
                            ) {
                                *self = state;
                            }
                        }
                    }
                },
            )
        });
    }

    fn attributes_edit_form(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Application::BiosAttributes {
                    root,
                    access_mode,
                    controls,
                    status,
                } = self
                {
                    let status = status.clone();
                    let mut changed = false;
                    ui.add_enabled_ui(access_mode.write_access(), |ui| {
                        egui::Grid::new("Attributes Grid")
                            .spacing([20f32, 5f32])
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                for control in controls {
                                    changed = ui.add(control.clone()).changed() || changed;
                                    ui.end_row();
                                }
                            });
                    });
                    if changed {
                        Self::check_pending_reboot(root, &status);
                    }
                }
            });
    }

    fn header_bar(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |col| {
            col[0].horizontal(|ui| {
                ui.label(RichText::new("⚙").size(68.0));
                ui.heading("\n BIOS Configuration Tool\n");
            });
            col[1].vertical(|ui| match self {
                Application::BiosAttributes {
                    root,
                    status,
                    access_mode: AccessMode::ReadWriteAuthenticated(auth),
                    ..
                } => {
                    ui.label(format!("Logged in: {}", auth.login));
                    if ui.button("Logout").clicked() {
                        let _ = auth.authenticate_with_password("");
                        if let Some(state) = status.handle_result_with_message(
                            Self::bios_admin_authentication(root, status),
                            "Logged out",
                        ) {
                            *self = state;
                        }
                    }
                }
                Application::BiosAttributes {
                    root,
                    status,
                    access_mode: AccessMode::ReadOnly,
                    ..
                } => {
                    ui.label("Not logged in");
                    if ui.button("Login").clicked() {
                        if let Some(state) = status.handle_result_with_message(
                            Self::bios_admin_authentication(root, status),
                            "Logged out",
                        ) {
                            *self = state;
                        }
                    }
                }
                Application::BiosAttributes {
                    access_mode: AccessMode::ReadWrite,
                    ..
                } => {
                    ui.label("Not logged in");
                    ui.label("BIOS not protected");
                }
                _ => {}
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let status = self.status();
        let inner = status.inner();
        if inner.reboot_required {
            ui.horizontal(|ui| {
                ui.small("Changes will be applied after restart.");
                if ui.small_button("Reboot").clicked() {
                    status.handle_result_with_message(system_shutdown::reboot(), "Rebooting...");
                }
            });
            ui.separator();
        }
        ui.horizontal(|ui| {
            ui.small(inner.changed.format("%d/%m/%Y %H:%M:%S").to_string());
            match inner.message {
                StatusMessage::Ok => {
                    ui.small("Ok");
                }
                StatusMessage::Message(msg) => {
                    ui.small(msg);
                }
                StatusMessage::Error(err) => {
                    ui.small(RichText::new("Error: ").color(ui.style().visuals.error_fg_color));
                    ui.small(err);
                }
            }
        });
    }

    fn select_root_ui(&mut self, ui: &mut egui::Ui) {
        if let Application::SelectRoot { roots, status } = self {
            if roots.is_empty() {
                ui.label("Firmware Attributes root not found");
            } else {
                let mut selected: Option<&PathBuf> = None;
                egui::ComboBox::from_id_source("Select Root")
                    .selected_text("Select Firmware Attributes root")
                    .show_ui(ui, |ui| {
                        for root in roots {
                            ui.selectable_value(&mut selected, Some(root), format!("{:?}", root));
                        }
                    });
                if let Some(root) = selected {
                    let state = status.handle_result_with_message(
                        Self::bios_admin_authentication(&root, status),
                        &format!("Root: {:?}", root),
                    );
                    if let Some(state) = state {
                        *self = state;
                    }
                }
            }
        }
    }
}
