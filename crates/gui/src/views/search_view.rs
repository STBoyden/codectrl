use iced::{
	widget::{container, text},
	Command, Element,
};

use crate::{view::View, Message, ViewState};

#[derive(Debug, Clone, Default)]
pub struct Searching {
	pub filter: String,
	pub case_sensitive: bool,
	pub regex_sensitive: bool,
}

impl View for Searching {
	type Message = Message;

	fn title(&self) -> String { String::from("Searching logs...") }

	fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
		use Message::*;

		match message {
			FilterTextChanged(text) => {
				self.filter = text;

				if self.filter.is_empty() {
					return self.send_message(Message::UpdateViewState(ViewState::Main));
				}

				self.send_message(Message::UpdateViewState(ViewState::Searching))
			},
			ClearFilterText => self.send_message(Message::FilterTextChanged(String::new())),
			FilterCaseSensitivityChanged(state) => {
				self.case_sensitive = state;
				Command::none()
			},
			FilterRegexChanged(state) => {
				self.regex_sensitive = state;
				Command::none()
			},
			_ => Command::none(),
		}
	}

	fn view(&self) -> Element<'_, Self::Message> {
		container(text(format!("Searching view: {}", self.filter)))
			.padding(10.0)
			.into()
	}
}
