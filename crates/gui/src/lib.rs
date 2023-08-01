#![feature(associated_type_defaults)]
#![warn(clippy::perf, clippy::pedantic)]
#![allow(clippy::enum_glob_use)]

mod view;
mod views;

use crate::view::View;

use anyhow::Error;
use codectrl_protobuf_bindings::{
	data::Log,
	logs_service::{log_server_client::LogServerClient, Connection, RequestStatus, ServerDetails},
};
use codectrl_server::{self, ServerResult};
use dark_light::{self, Mode as ThemeMode};
pub use iced;
use iced::{
	executor, subscription,
	widget::{button, checkbox, column, container, row, text, text_input, Rule},
	window::close,
	Alignment, Application, Command, Element, Length, Subscription, Theme,
};
use iced_aw::{
	helpers::{menu_bar, menu_tree},
	menu::PathHighlight,
	menu_tree, quad,
	split::Axis,
	Split,
};
use iced_native::futures::StreamExt;
use parking_lot::Mutex;
use std::{
	borrow::Cow,
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tonic::{transport::Channel, Response, Status, Streaming};

pub enum PauseState {
	Paused,
	InProgress,
}

type Client = LogServerClient<Channel>;

pub enum GrpcConnection {
	NotConnected(String, u32),
	Connected(Client, Option<Connection>),
	FetchingDetails(Client, Connection),
	Registered(Client, Connection),
	Streaming(
		(Option<Result<Log, Status>>, Streaming<Log>),
		Client,
		Connection,
	),
	Error(Status, Client, Option<Connection>),
}

#[derive(Debug, Clone)]
pub enum Message {
	// main view
	LogAppearanceStateChanged,
	LogClicked(Log),
	LogIndexChanged(Option<Cow<'static, str>>),
	LogDetailsSplitResize(u16),
	UpdateLogItems(Box<Self>),
	LogDetailsSplitClose,

	// searching view
	FilterTextChanged(String),
	ClearFilterText,
	FilterCaseSenitivityChanged(bool),
	FilterRegexChanged(bool),

	// general
	UpdateViewState(ViewState),
	SplitResize(u16),
	NoOp,
	Quit,

	// server-related
	ServerStarted {
		server_result: Arc<ServerResult>,
		details: (String, u32),
	},
	GetConnectionDetails(Client, Option<ServerDetails>),
	SetConnectionDetails(Arc<Option<Response<ServerDetails>>>),
	ShowServerErrors,
	ShowServerError(Arc<anyhow::Error>),
	SetServerErrorChannel(Arc<mpsc::UnboundedReceiver<anyhow::Error>>),
	GetServerErrors(Arc<Mutex<mpsc::UnboundedReceiver<anyhow::Error>>>),
	AddServerError(Option<Arc<anyhow::Error>>),
	SortLogs,
	ServerAddLog(Log),
}

#[derive(Debug, Eq, PartialEq, Clone, Default)]
pub enum ViewState {
	Searching,
	#[default]
	Main,
}

fn separator<'a, Message>() -> iced_aw::menu::menu_tree::MenuTree<'a, Message, iced::Renderer> {
	menu_tree!(quad::Quad {
		color: [0.5; 3].into(),
		border_radius: 4.0.into(),
		inner_bounds: quad::InnerBounds::Ratio(0.98, 0.1),
		..Default::default()
	})
}

#[derive(Debug, Clone)]
pub struct Flags {
	host: String,
	port: u32,
}

impl Default for Flags {
	fn default() -> Self {
		Self {
			host: String::from("127.0.0.1"),
			port: 3002,
		}
	}
}

#[derive(Debug, Clone)]
pub struct App {
	// server communication
	server_errors_channel: Option<Arc<Mutex<mpsc::UnboundedReceiver<anyhow::Error>>>>,
	server_errors: Vec<Arc<anyhow::Error>>,
	host: String,
	port: u32,
	uptime: Duration,
	last_updated: Instant,

	// splits
	split_size: Option<u16>,

	// views and view state
	view_state: ViewState,
	main_view: views::Main,
	searching_view: views::Searching,
}

impl Default for App {
	fn default() -> Self { Self::new_no_server(Flags::default()) }
}

impl App {
	fn new_no_server(flags: Flags) -> Self {
		Self {
			host: flags.host,
			port: flags.port,
			uptime: Duration::ZERO,
			last_updated: Instant::now(),
			server_errors: vec![],
			server_errors_channel: None,
			split_size: Some(208),
			view_state: ViewState::default(),
			main_view: views::Main::default(),
			searching_view: views::Searching::default(),
		}
	}

	fn send_message(&self, message: Message) -> Command<Message> {
		Command::perform(async {}, |_| message)
	}
}

impl Application for App {
	type Message = Message;
	type Executor = executor::Default;
	type Theme = Theme;
	type Flags = Flags;

	fn new(flags: Self::Flags) -> (Self, Command<Message>) {
		(
			App::default(),
			Command::perform(
				codectrl_server::run_server(
					Some(flags.host.clone()),
					Some(flags.port),
					None,
					None,
					false,
				),
				move |result| Message::ServerStarted {
					server_result: Arc::new(result),
					details: (flags.host, flags.port),
				},
			),
		)
	}

	fn title(&self) -> String { String::from("CodeCTRL") }

	fn update(&mut self, message: Self::Message) -> Command<Message> {
		use Message::*;

		match message {
			LogAppearanceStateChanged
			| ServerAddLog(_)
			| LogClicked(_)
			| LogDetailsSplitResize(_)
			| LogIndexChanged(_)
			| UpdateLogItems(_)
			| SortLogs
			| LogDetailsSplitClose => self.main_view.update(message),

			FilterTextChanged(_)
			| ClearFilterText
			| FilterCaseSenitivityChanged(_)
			| FilterRegexChanged(_) => self.searching_view.update(message),

			UpdateViewState(state) => {
				self.view_state = state;
				Command::none()
			},
			SplitResize(size) => {
				self.split_size = Some(size);
				Command::none()
			},
			ServerStarted {
				server_result,
				details: (host, port),
			} => match Arc::try_unwrap(server_result) {
				Ok(x) if x.is_ok() => {
					let channel = x.unwrap();
					self.host = host;
					self.port = port;

					self.send_message(SetServerErrorChannel(Arc::new(channel)))
				},
				Ok(x) if x.is_err() => {
					let error = x.unwrap_err();

					self.send_message(AddServerError(Some(Arc::new(error))))
				},
				Ok(_) => unreachable!(),
				Err(_) => self.send_message(AddServerError(Some(Arc::new(Error::msg(
					"Could not unwrap server result",
				))))),
			},
			SetServerErrorChannel(rx) => {
				if let Ok(rx) = Arc::try_unwrap(rx) {
					self.server_errors_channel = Some(Arc::new(Mutex::new(rx)));
				} else {
					return self.send_message(AddServerError(Some(Arc::new(anyhow::Error::msg(
						"Could not unwrap server error receiver",
					)))));
				}

				Command::none()
			},

			GetServerErrors(rx) => Command::perform(
				async move {
					let mut lock = rx.lock();
					if let Ok(msg) = lock.try_recv() {
						Some(Arc::new(msg))
					} else {
						None
					}
				},
				|msg| AddServerError(msg),
			),
			AddServerError(error) => {
				if let Some(error) = error {
					self.server_errors.push(error);
				}

				self.send_message(ShowServerErrors)
			},
			GetConnectionDetails(mut client, details) => match details {
				Some(details) =>
					self.send_message(SetConnectionDetails(Arc::new(Some(Response::new(details))))),
				None if Instant::now().duration_since(self.last_updated).as_secs() >= 1 =>
					Command::perform(
						async move { Arc::new(client.get_server_details(()).await.ok()) },
						SetConnectionDetails,
					),
				None => Command::none(),
			},
			SetConnectionDetails(details) => {
				if let Some(details) = details.as_ref() {
					let details = details.get_ref();

					self.host = details.host.clone();
					self.port = details.port;

					if details.uptime != self.uptime.as_secs() {
						self.uptime = Duration::new(details.uptime, 0);
					}

					self.last_updated = Instant::now();
				}

				Command::none()
			},
			ShowServerErrors => {
				let get = if self.server_errors_channel.is_some() {
					let channel = self.server_errors_channel.as_ref().unwrap();
					let channel = Arc::clone(channel);

					self.send_message(GetServerErrors(channel))
				} else {
					Command::none()
				};

				let mut show = vec![];

				while let Some(current) = self.server_errors.pop() {
					show.push(current);
				}

				let show = Command::batch(
					show
						.iter()
						.map(|error| self.send_message(ShowServerError(Arc::clone(error)))),
				);

				Command::batch(vec![get, show])
			},

			ShowServerError(error) => {
				dbg!(error);
				Command::none()
			},
			NoOp => Command::none(),
			Quit => close(),
		}
	}

	fn subscription(&self) -> Subscription<Self::Message> {
		let mut batch = vec![];

		batch.push(subscription::unfold(
			"RefreshErrors",
			PauseState::InProgress,
			move |state| async move {
				match state {
					PauseState::Paused => {
						tokio::time::sleep(Duration::new(1, 0)).await;

						(Message::NoOp, PauseState::InProgress)
					},
					PauseState::InProgress => (Message::ShowServerErrors, PauseState::Paused),
				}
			},
		));

		batch.push(subscription::unfold(
			"GetLogs",
			GrpcConnection::NotConnected(self.host.clone(), self.port),
			move |state| async move {
				match state {
					GrpcConnection::NotConnected(host, port) => {
						let grpc_client = loop {
							let res = LogServerClient::connect(format!("http://{host}:{port}")).await;

							if let Ok(res) = res {
								break res;
							}
						};

						(Message::NoOp, GrpcConnection::Connected(grpc_client, None))
					},
					GrpcConnection::Connected(mut client, connection) => {
						if let Some(connection) = connection {
							match client.register_existing_client(connection.clone()).await {
								Ok(response) =>
									match RequestStatus::from_i32(response.into_inner().status).unwrap() {
										RequestStatus::Confirmed => (
											Message::GetConnectionDetails(client.clone(), None),
											GrpcConnection::FetchingDetails(client, connection),
										),
										RequestStatus::Error => todo!(),
									},
								Err(status) => (
									Message::NoOp,
									GrpcConnection::Error(status, client, Some(connection)),
								),
							}
						} else {
							match client.register_client(()).await {
								Ok(response) => (
									Message::GetConnectionDetails(client.clone(), None),
									GrpcConnection::FetchingDetails(client, response.into_inner()),
								),
								Err(status) => (
									Message::NoOp,
									GrpcConnection::Error(status, client, connection),
								),
							}
						}
					},
					GrpcConnection::FetchingDetails(mut client, connection) =>
						match client.get_server_details(()).await {
							Ok(details) => (
								Message::GetConnectionDetails(client.clone(), Some(details.into_inner())),
								GrpcConnection::Registered(client, connection),
							),
							Err(status) => (
								Message::GetConnectionDetails(client.clone(), None),
								GrpcConnection::Error(status, client, Some(connection)),
							),
						},
					GrpcConnection::Registered(mut client, connection) => {
						match client.get_logs(connection.clone()).await {
							Ok(res) => {
								let stream = res.into_inner();

								(
									Message::GetConnectionDetails(client.clone(), None),
									GrpcConnection::Streaming(stream.into_future().await, client, connection),
								)
							},
							Err(status) => (
								Message::GetConnectionDetails(client.clone(), None),
								GrpcConnection::Error(status, client, Some(connection)),
							),
						}
					},
					GrpcConnection::Streaming((log, tail), client, connection) => match log {
						Some(log) => match log {
							Ok(log) => (
								Message::ServerAddLog(log),
								GrpcConnection::Streaming(tail.into_future().await, client, connection),
							),
							Err(status) => (
								Message::GetConnectionDetails(client.clone(), None),
								GrpcConnection::Error(status, client, Some(connection)),
							),
						},
						None => (
							Message::GetConnectionDetails(client.clone(), None),
							GrpcConnection::Registered(client, connection),
						),
					},
					GrpcConnection::Error(status, client, connection) => {
						let code = status.code().clone();
						let message = status.message().to_string();

						match code {
							tonic::Code::Ok | tonic::Code::ResourceExhausted if connection.is_some() => {
								// tokio::time::sleep(Duration::new(5, 0)).await;
								(
									Message::NoOp,
									GrpcConnection::Registered(client, connection.unwrap()),
								)
							},
							_ => (
								Message::AddServerError(Some(Arc::new(anyhow::Error::msg(message)))),
								GrpcConnection::Connected(client, connection),
							),
						}
					},
				}
			},
		));

		Subscription::batch(batch)
	}

	fn view(&self) -> Element<Self::Message> {
		let file_menu_tree = menu_tree(
			button("File"),
			vec![
				menu_tree!(button("Save project").width(Length::Fill)),
				menu_tree!(button("Open project").width(Length::Fill)),
				separator(),
				menu_tree!(button("Settings").width(Length::Fill)),
				menu_tree!(button("Log out").width(Length::Fill)),
				separator(),
				menu_tree!(button("Quit").width(Length::Fill).on_press(Message::Quit)),
			],
		);

		let help_menu_tree = menu_tree(
			button("Help"),
			vec![menu_tree!(button("About").width(Length::Fill))],
		);

		let menu_bar = menu_bar(vec![file_menu_tree, help_menu_tree])
			.path_highlight(Some(PathHighlight::Full))
			.spacing(1.0)
			.padding(2.0);

		let side_bar = container(
			column![
				text_input("Filter", &self.searching_view.filter).on_input(Message::FilterTextChanged),
				button("Clear").on_press(Message::ClearFilterText),
				checkbox(
					"Case sensitive",
					self.searching_view.case_sensitive,
					Message::FilterCaseSenitivityChanged
				),
				checkbox(
					"Regex",
					self.searching_view.regex_sensitive,
					Message::FilterRegexChanged
				),
				Rule::horizontal(1.0),
				row![
					text("Sort logs by: "),
					button(text(&self.main_view.log_appearance)).on_press(Message::LogAppearanceStateChanged)
				]
				.align_items(Alignment::Center),
				Rule::horizontal(1.0),
				text(format!("Server address: {}:{}", self.host, self.port)),
				text(format!("Server uptime: {}s", self.uptime.as_secs())),
			]
			.align_items(Alignment::Start)
			.spacing(4.0)
			.padding(10.0),
		);

		column![
			menu_bar,
			Rule::horizontal(1.0),
			row![
				Split::new(
					side_bar.width(Length::Fill),
					container(match self.view_state {
						ViewState::Main => self.main_view.view(),
						ViewState::Searching => self.searching_view.view(),
					})
					.width(Length::Fill),
					self.split_size,
					Axis::Vertical,
					Message::SplitResize
				)
				.min_size_first(208)
				.min_size_second(600)
			]
		]
		.into()
	}

	fn theme(&self) -> Theme {
		let mode = dark_light::detect();

		match mode {
			ThemeMode::Dark | ThemeMode::Default => Theme::Dark,
			ThemeMode::Light => Theme::Light,
		}
	}
}
