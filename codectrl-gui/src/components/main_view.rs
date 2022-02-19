use super::{
    main_view_components::draw_log_item,
    regex_filter,
    theming::{CODECTRL_GREEN, DARK_HEADER_FOREGROUND_COLOUR},
};
use crate::data::{AppState, Filter};

use chrono::{DateTime, Local};
use codectrl_logger::Log;
use egui::{CtxRef, RichText};
use regex::RegexBuilder;

fn app_state_filter(
    is_case_sensitive: bool,
    is_using_regex: bool,
    search_filter: &str,
    filter_by: &Filter,
    log: &Log<String>,
    time: &DateTime<Local>,
) -> bool {
    let string_filter = |search_filter: &str, search_string: &str| -> bool {
        if is_case_sensitive {
            log.message.contains(search_filter)
        } else if is_using_regex {
            regex_filter(search_filter, search_string, is_case_sensitive)
        } else {
            log.message
                .to_lowercase()
                .contains(&search_filter.to_lowercase())
        }
    };

    match filter_by {
        Filter::Message => string_filter(search_filter, &log.message),
        Filter::Time => time.format("%F %X").to_string().contains(&*search_filter),
        Filter::FileName => string_filter(search_filter, &log.file_name),
        Filter::Address => {
            let regex = if let Ok(regex) =  RegexBuilder::new(
                r#"(\b25[0-5]|\b2[0-4][0-9]|\b[01]?[0-9][0-9]?)(\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)){3}"#,
            ).build() {
                regex
            } else {
                return false;
            };

            let address = log.address.split('.');
            let search_address = search_filter.split('.');
            let mut is_matching = true;
            let mut contains_glob = false;

            for (address_split, search_split) in address.zip(search_address) {
                if search_split == "*" {
                    contains_glob = true;
                    continue;
                }

                if !is_matching {
                    break;
                }

                match (address_split.parse::<u8>(), search_split.parse::<u8>()) {
                    (Ok(ap), Ok(sp)) => is_matching = ap == sp,
                    (Err(_), _) | (_, Err(_)) => return false,
                }
            }

            if !contains_glob
                && (!regex.is_match(&log.address) || !regex.is_match(search_filter))
            {
                return false;
            }

            is_matching
        },
        Filter::LineNumber => {
            let number = search_filter.parse::<u32>().unwrap_or(0);

            if number == 0 {
                return true;
            }

            log.line_number == number
        },
    }
}

pub fn main_view(app_state: &mut AppState, ctx: &CtxRef) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            egui::ScrollArea::vertical()
                .max_width(ui.available_width())
                .max_height(ui.available_height() - app_state.preview_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("received_grid")
                        .striped(true)
                        .spacing((0.0, 10.0))
                        .min_col_width(ui.available_width() / 6.0)
                        .max_col_width(ui.available_width() / 6.0)
                        .show(ui, |ui| {
                            ui.heading("");
                            ui.heading(
                                RichText::new("Message")
                                    .color(DARK_HEADER_FOREGROUND_COLOUR),
                            );
                            ui.heading(
                                RichText::new("Host")
                                    .color(DARK_HEADER_FOREGROUND_COLOUR),
                            );
                            ui.heading(
                                RichText::new("File name")
                                    .color(DARK_HEADER_FOREGROUND_COLOUR),
                            );
                            ui.heading(
                                RichText::new("Line number")
                                    .color(DARK_HEADER_FOREGROUND_COLOUR),
                            );
                            ui.heading(
                                RichText::new("Date & time")
                                    .color(DARK_HEADER_FOREGROUND_COLOUR),
                            );
                            ui.end_row();

                            let received_vec = app_state.received.read().unwrap();
                            let mut received_vec: Vec<_> = received_vec.iter().collect();

                            received_vec.sort_by(|(_, a_time), (_, b_time)| {
                                if app_state.is_newest_first {
                                    b_time.partial_cmp(a_time).unwrap()
                                } else {
                                    a_time.partial_cmp(b_time).unwrap()
                                }
                            });

                            for received in received_vec.iter().filter(|(log, time)| {
                                app_state_filter(
                                    app_state.is_case_sensitive,
                                    app_state.is_using_regex,
                                    &app_state.search_filter,
                                    &app_state.filter_by,
                                    log,
                                    time,
                                )
                            }) {
                                draw_log_item(
                                    &app_state.message_alerts,
                                    &mut app_state.clicked_item,
                                    app_state.do_scroll_to_selected_log,
                                    received,
                                    ui,
                                );
                            }
                        });
                });
        });
    });
}

pub fn main_view_empty(ctx: &CtxRef, socket_address: &str) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.small(RichText::new("codeCTRL").color(CODECTRL_GREEN));
            ui.heading(RichText::new("by pwnCTRL").italics());
            ui.add_space(ui.available_height() / 3.0);

            ui.heading(format!(
                "Send logs to {} and they will appear here.",
                socket_address
            ));
        });
    });
}