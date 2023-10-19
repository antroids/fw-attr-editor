// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::application::Status;
use crate::sysfs_firmware_attributes::{
    Attribute, AttributeParser, ReadableAttribute, WriteableAttribute,
};
use egui::Widget;
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct Control<T: AttributeParser> {
    status: Status,
    attribute: T::Attr,
}

impl Control<Attribute> {
    pub fn new(attribute: Attribute, status: &Status) -> Self {
        Self {
            attribute,
            status: status.clone(),
        }
    }

    fn current_value<T>(&self, attr: &dyn ReadableAttribute<Value = T>) -> Option<T> {
        self.status.handle_result(attr.current_value())
    }

    fn write_current_value<T: Debug + PartialEq>(
        &self,
        attr: &dyn WriteableAttribute<Value = T>,
        value: &T,
    ) {
        if let Ok(current) = attr.current_value() {
            if value == &current {
                return;
            }
        }
        self.status.handle_result_with_message(
            attr.write_current_value(value),
            &format!(
                "Value updated for Attribute {:?} to {:?}",
                attr.common_attribute().display_name(),
                value
            ),
        );
    }
}

impl Widget for Control<Attribute> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut changed = false;
        match &self.attribute {
            Attribute::Enumeration(attr) => {
                if let Some(mut current_value) = self.current_value(attr) {
                    let name = attr.common_attribute().display_name();
                    if ui
                        .add(enumeration_combobox(
                            name,
                            &mut current_value,
                            &attr.possible_values,
                        ))
                        .changed()
                    {
                        changed = true;
                        self.write_current_value(attr, &current_value);
                    }
                }
            }
            Attribute::Integer(attr) => {
                if let Some(mut current_value) = self.current_value(attr) {
                    let name = attr.common_attribute().display_name();
                    if ui
                        .add(integer_input(
                            name,
                            &mut current_value,
                            attr.min_value,
                            attr.max_value,
                            attr.scalar_increment,
                        ))
                        .changed()
                    {
                        changed = true;
                        self.write_current_value(attr, &current_value);
                    }
                }
            }
            Attribute::String(attr) => {
                if let Some(current_value) = self.current_value(attr) {
                    let id = ui.id();
                    let mut current_value = ui
                        .memory(|mem| mem.data.get_temp(id))
                        .unwrap_or(current_value);
                    let name = attr.common_attribute().display_name();
                    let input_response = ui.add(string_input(
                        name,
                        &mut current_value,
                        attr.min_length,
                        attr.max_length,
                        attr.hint.as_ref().unwrap_or(&"".to_string()),
                    ));
                    if input_response.lost_focus()
                        || (input_response.has_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        changed = true;
                        self.write_current_value(attr, &current_value);
                        ui.memory_mut(|mem| mem.data.remove::<String>(id));
                    } else if input_response.has_focus() {
                        ui.memory_mut(|mem| mem.data.insert_temp(id, current_value));
                    }
                }
            }
            Attribute::OrderedList(attr) => {
                if let Some(mut current_value) = self.current_value(attr) {
                    let name = attr.common_attribute().display_name();
                    if ui
                        .add(ordered_list_widget(
                            name,
                            &mut current_value,
                            &attr.elements,
                        ))
                        .changed()
                    {
                        changed = true;
                        self.write_current_value(attr, &current_value);
                    }
                }
            }
            Attribute::EnumerationList(attr) => {
                if let Some(mut current_value) = self.current_value(attr) {
                    let name = attr.common_attribute().display_name();
                    if ui
                        .add(ordered_list_widget(
                            name,
                            &mut current_value,
                            &attr.possible_values,
                        ))
                        .changed()
                    {
                        changed = true;
                        self.write_current_value(attr, &current_value);
                    }
                }
            }
        };
        let mut response = ui.label("");
        if changed {
            response.mark_changed();
        }
        response
    }
}

fn enumeration_combobox<'a>(
    name: &'a str,
    current_value: &'a mut String,
    possible_values: &'a Vec<String>,
) -> impl Widget + 'a {
    move |ui: &mut egui::Ui| -> egui::Response {
        let before = current_value.clone();
        ui.label(name);
        let mut response = egui::ComboBox::from_id_source(name)
            .selected_text(current_value.as_str())
            .show_ui(ui, |ui| {
                for variant in possible_values {
                    ui.selectable_value(current_value, variant.clone(), variant);
                }
            })
            .response;
        if before != *current_value {
            response.mark_changed();
        }
        response
    }
}

fn integer_input<'a>(
    name: &'a str,
    current_value: &'a mut i32,
    min: i32,
    max: i32,
    step: i32,
) -> impl Widget + 'a {
    move |ui: &mut egui::Ui| -> egui::Response {
        ui.label(name);
        egui::Slider::new(current_value, min..=max)
            .step_by(step as f64)
            .clamp_to_range(true)
            .ui(ui)
    }
}

fn string_input<'a>(
    name: &'a str,
    current_value: &'a mut String,
    _min_length: usize,
    max_length: usize,
    hint: &'a str,
) -> impl Widget + 'a {
    move |ui: &mut egui::Ui| -> egui::Response {
        ui.label(name);
        let response = egui::TextEdit::singleline(current_value)
            .char_limit(max_length)
            .ui(ui);
        ui.weak(hint);
        response
    }
}

fn ordered_list_widget<'a>(
    name: &'a str,
    current_value: &'a mut Vec<String>,
    possible_values: &'a Vec<String>,
) -> impl Widget + 'a {
    move |ui: &mut egui::Ui| -> egui::Response {
        let before = current_value.clone();
        ui.label(name);
        let mut response = ui
            .vertical(|ui| {
                let len = current_value.len();
                for (index, value) in current_value.clone().iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        if ui.small_button("⬆").clicked() {
                            if index == 0 {
                                current_value.swap(index, len - 1);
                            } else {
                                current_value.swap(index, index - 1);
                            }
                        }
                        if ui.small_button("⬇").clicked() {
                            if index == len - 1 {
                                current_value.swap(index, 0);
                            } else {
                                current_value.swap(index, index + 1);
                            }
                        }
                        if ui.small_button("❌").clicked() {
                            current_value.remove(index);
                        }
                        ui.label(value.as_str());
                    });
                }
                if !possible_values.is_empty() {
                    ui.separator();
                    let mut selected: Option<&String> = None;
                    egui::ComboBox::from_id_source(name)
                        .selected_text("Add to list")
                        .show_ui(ui, |ui| {
                            for possible_value in possible_values {
                                ui.selectable_value(
                                    &mut selected,
                                    Some(possible_value),
                                    possible_value,
                                );
                            }
                        });
                    if let Some(selected) = selected {
                        current_value.push(selected.clone());
                    }
                }
            })
            .response;
        if before != *current_value {
            response.mark_changed();
        }
        response
    }
}
