#![allow(dead_code)]

use eframe::{
    egui::{
        self, CollapsingHeader, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText,
        TextFormat, TextStyle,
    },
    epaint::{ahash::HashMap, text::LayoutJob, Vec2},
    CreationContext,
};
use egui_extras::RetainedImage;
use poll_promise::Promise;
use serde::Deserialize;
use time::OffsetDateTime;
use url::Url;

mod comment_parser;
mod fetch_favicon;

pub const DEBUG_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F12);
pub const REFRESH_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);

#[derive(Deserialize, Clone)]
#[serde(default)]
struct HnItem {
    id: usize,
    deleted: bool,
    r#type: String,
    by: String,
    #[serde(with = "time::serde::timestamp")]
    time: OffsetDateTime,
    text: String,
    dead: bool,
    parent: usize,
    poll: usize,
    kids: Vec<usize>,
    url: Option<Url>,
    score: usize,
    title: String,
    parts: Vec<usize>,
    descendants: usize, // comment count when type is story
}

impl Default for HnItem {
    fn default() -> Self {
        Self {
            id: Default::default(),
            deleted: Default::default(),
            r#type: Default::default(),
            by: Default::default(),
            time: OffsetDateTime::now_utc(),
            text: Default::default(),
            dead: Default::default(),
            parent: Default::default(),
            poll: Default::default(),
            kids: Default::default(),
            url: Default::default(),
            score: Default::default(),
            title: Default::default(),
            parts: Default::default(),
            descendants: Default::default(),
        }
    }
}

#[derive(Default)]
struct Application {
    stories: Vec<HnItem>,
    loading_items: Option<Vec<Promise<ehttp::Result<HnItem>>>>,
    status: RequestStatus,
    load_amount: Option<usize>,
    story_comments: Option<HnItem>,
    items: HashMap<usize, Promise<ehttp::Result<HnItem>>>,
    favicons: HashMap<Url, Promise<ehttp::Result<RetainedImage>>>,
    page: Page,
    default_icon: Option<RetainedImage>,
    y_icon: Option<RetainedImage>,
    render_html: bool,
    show_debug_window: bool,
    text_input: String,
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
enum Page {
    #[default]
    Top,
    New,
    Show,
    Ask,
    Jobs,
}

fn fetch_url<T>(ctx: egui::Context, url: &str) -> Promise<ehttp::Result<T>>
where
    T: serde::de::DeserializeOwned + Send,
{
    let (sender, promise) = Promise::new();
    let request = ehttp::Request::get(url);
    ehttp::fetch(request, move |response| {
        ctx.request_repaint(); // wake up UI thread

        if let Err(err) = response {
            sender.send(Err(err));
            return;
        }

        let response = response.unwrap();

        match serde_json::from_slice::<T>(&response.bytes) {
            Ok(value) => sender.send(Ok(value)),
            Err(err) => sender.send(Err(format!("Could not deserialize response: {}", err))),
        }
    });

    promise
}

#[rustfmt::skip]
fn fetch_page_stories(page: Page, ctx: egui::Context) -> Promise<ehttp::Result<Vec<usize>>> {
    match page {
        Page::Top => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/topstories.json"),
        Page::New => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/newstories.json"),
        Page::Show => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/showstories.json"),
        Page::Ask => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/askstories.json"),
        Page::Jobs => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/jobstories.json"),
    }
}

fn fetch_item(ctx: egui::Context, item_id: usize) -> Promise<ehttp::Result<HnItem>> {
    // https://hacker-news.firebaseio.com/v0/item/8863.json
    fetch_url(
        ctx,
        &format!("https://hacker-news.firebaseio.com/v0/item/{item_id}.json"),
    )
}

fn configure_styles(ctx: &egui::Context) {
    use egui::FontFamily::{Monospace, Proportional};

    let mut style = (*ctx.style()).clone();

    style.text_styles = [
        (TextStyle::Small, FontId::new(8.0, Proportional)),
        (TextStyle::Body, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, Monospace)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Heading, FontId::new(22.0, Proportional)),
    ]
    .into();

    // give buttons a little bit of breathing room
    style.spacing.button_padding = Vec2::splat(5.0);

    ctx.set_style(style);
}

fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();

    const HN_ORANGE: Color32 = Color32::from_rgb(0xff, 0x6d, 0x00);

    // #f6f6ef

    // visuals.window_fill = Color32::BROWN;
    // visuals.widgets.noninteractive.bg_fill = Color32::BROWN;

    // the background of central panel
    visuals.panel_fill = Color32::from_rgb(0xf6, 0xf6, 0xef);

    // the background of scrollbar behind the handle
    visuals.extreme_bg_color = Color32::from_rgb(0xf6, 0xf6, 0xef);

    // hacker news orange color
    visuals.hyperlink_color = HN_ORANGE;

    // colors when selectable_value is selected
    visuals.selection.bg_fill = HN_ORANGE;
    visuals.selection.stroke.color = Color32::WHITE;

    ctx.set_visuals(visuals);
}

fn rich_text_with_style(text: impl Into<String>, style: &comment_parser::TextStyle) -> RichText {
    let mut rich_text = RichText::new(text);

    if style.italic {
        rich_text = rich_text.italics();
    }

    if style.monospace {
        rich_text = rich_text.monospace();
    }

    rich_text
}

impl Application {
    fn new(cc: &CreationContext) -> Self {
        configure_visuals(&cc.egui_ctx);
        configure_styles(&cc.egui_ctx);

        let status = RequestStatus::Loading(fetch_page_stories(Page::Top, cc.egui_ctx.clone()));

        let default_icon = RetainedImage::from_image_bytes(
            "default_icon",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/default_icon.png" // https://icons8.com/icon/NyuxPErq0tu2/globe-africa
            )),
        )
        .unwrap();

        let y_icon = RetainedImage::from_image_bytes(
            "y_icon",
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/y_icon.png")),
        )
        .unwrap();

        Self {
            status,
            default_icon: Some(default_icon),
            y_icon: Some(y_icon),
            render_html: true,
            ..Default::default()
        }
    }

    fn render_html_text(&self, text: &str, ui: &mut egui::Ui) {
        if !self.render_html {
            ui.label(text);
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;

            let parser = comment_parser::Parser::new(text);
            for (item, style) in parser {
                match item {
                    comment_parser::Item::Escape(c) => {
                        ui.label(rich_text_with_style(c.to_string(), &style));
                    }
                    comment_parser::Item::Text(text) => {
                        ui.label(rich_text_with_style(text, &style));
                    }
                    comment_parser::Item::NewLine => {
                        ui.label("\n");
                    }
                    comment_parser::Item::Link(mut url, mut text) => {
                        let url = url.to_string();
                        let text = text.to_string();
                        ui.hyperlink_to(rich_text_with_style(text, &style), url);
                    }
                }
            }
        });
    }

    fn render_story(
        &self,
        story: &HnItem,
        ui: &mut egui::Ui,
        show_text: bool,
        can_open_comments: bool,
    ) -> bool {
        enum Intent {
            OpenComments,
            OpenLink,
        }

        let comment_link_enabled = story.descendants > 0 && can_open_comments;
        let link_enabled = story.url.is_some() || comment_link_enabled;
        let mut intent = None;

        if let Some(url) = &story.url {
            ui.horizontal(|ui| {
                if let Some(promise) = self.favicons.get(url) {
                    if let Some(result) = promise.ready() {
                        let image = result
                            .as_ref()
                            .ok()
                            .unwrap_or_else(|| self.default_icon.as_ref().unwrap());
                        let height = ui.available_height();
                        image.show_size(ui, Vec2::new(height, height));
                    } else {
                        ui.spinner();
                    }
                } else {
                    ui.label("?");
                }
                ui.label(RichText::new(format_url(url)).monospace());
            });
        }

        let title_text = RichText::new(&story.title).heading().strong();
        if link_enabled {
            ui.scope(|ui| {
                ui.visuals_mut().hyperlink_color = ui.visuals().widgets.active.fg_stroke.color;
                if ui.link(title_text).clicked() {
                    intent = Some(Intent::OpenLink);
                }
            });
        } else {
            ui.label(title_text);
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new(&story.by).strong());
            ui.label("•");
            ui.label(RichText::new(format_date_time(&story.time)).weak());
        });

        if show_text && story.text.len() > 0 {
            self.render_html_text(&story.text, ui);
        }

        ui.horizontal(|ui| {
            if let Some(points_str) = format_points(story.score) {
                ui.label(&points_str);
                ui.label("•");
            }

            ui.add_enabled_ui(comment_link_enabled, |ui| {
                if ui.link(format_comments(story.descendants)).clicked() {
                    intent = Some(Intent::OpenComments);
                }
            });
        });

        // If there is url set and the intent is to open the link then open the url
        // otherwise if whatever intent is set meaning we are able to interact, then
        // open comments, this is so stories without url open comment section when
        // they click the title
        match (&story.url, intent) {
            (Some(url), Some(Intent::OpenLink)) => {
                ui.output_mut(|o| o.open_url(url));
                false
            }
            (_, Some(_)) => true,
            _ => false,
        }
    }

    fn render_comment(&mut self, comment_id: usize, ctx: &egui::Context, ui: &mut egui::Ui) {
        let promise = match self.items.remove(&comment_id) {
            Some(promise) => promise,
            None => fetch_item(ctx.clone(), comment_id),
        };

        if let Some(result) = promise.ready() {
            match result {
                Ok(comment) => {
                    let mut text_layout = LayoutJob::default();
                    if comment.by.len() > 0 {
                        text_layout.append(
                            &comment.by,
                            0.0,
                            TextFormat::simple(
                                FontId::proportional(16.0),
                                ui.visuals().strong_text_color(),
                            ),
                        );
                        text_layout.append(
                            "  •  ",
                            0.0,
                            TextFormat::simple(
                                FontId::proportional(16.0),
                                ui.visuals().weak_text_color(),
                            ),
                        );
                    }
                    text_layout.append(
                        &format_date_time(&comment.time),
                        0.0,
                        TextFormat::simple(
                            FontId::proportional(16.0),
                            ui.visuals().weak_text_color(),
                        ),
                    );

                    CollapsingHeader::new(text_layout)
                        .id_source(comment.id)
                        .default_open(true)
                        .show(ui, |ui| {
                            if comment.deleted {
                                ui.label("[deleted]");
                            } else {
                                if !self.render_html {
                                    if ui.small_button("Copy").clicked() {
                                        ui.output_mut(|o| o.copied_text = comment.text.to_string());
                                    }
                                }

                                self.render_html_text(&comment.text, ui);
                            }

                            egui::Frame::none()
                                .outer_margin(egui::style::Margin {
                                    left: 20f32,
                                    ..Default::default()
                                })
                                .show(ui, |ui| {
                                    for child in &comment.kids {
                                        self.render_comment(*child, ctx, ui);
                                    }
                                });
                        });
                }
                Err(error) => {
                    ui.label(format!("Error: {}", error));
                }
            };
        } else {
            ui.spinner();
        }

        self.items.insert(comment_id, promise);
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.status = RequestStatus::Loading(fetch_page_stories(self.page, ctx.clone()));
    }
}

fn format_date_time(date_time: &OffsetDateTime) -> String {
    let duration = OffsetDateTime::now_utc() - date_time.clone();

    if duration.whole_minutes() < 60 {
        if duration.whole_minutes() == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", duration.whole_minutes())
        }
    } else if duration.whole_hours() < 24 {
        if duration.whole_hours() == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", duration.whole_hours())
        }
    } else {
        if duration.whole_days() == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", duration.whole_days())
        }
    }
}

fn format_points(points: usize) -> Option<String> {
    match points {
        0 => None,
        1 => Some("1 point".to_string()),
        n => Some(format!("{} points", n)),
    }
}

fn format_url(url: &Url) -> String {
    url.host_str()
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| url.to_string())
}

fn format_comments(count: usize) -> String {
    match count {
        0 => "No comments".to_string(),
        1 => "1 comment".to_string(),
        n => format!("{} comments", n),
    }
}

enum RequestStatus {
    Done(Vec<usize>),
    Loading(Promise<ehttp::Result<Vec<usize>>>),
    Error(String),
}

impl Default for RequestStatus {
    fn default() -> Self {
        RequestStatus::Done(Vec::new())
    }
}

impl eframe::App for Application {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input_mut(|i| i.consume_shortcut(&DEBUG_SHORTCUT)) {
            self.show_debug_window = !self.show_debug_window;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&REFRESH_SHORTCUT)) {
            self.refresh(&ctx);
        }

        self.status = match std::mem::take(&mut self.status) {
            RequestStatus::Done(items) => RequestStatus::Done(items),
            RequestStatus::Loading(mut promise) => {
                if let Some(result) = promise.ready_mut() {
                    match result {
                        Ok(resource) => RequestStatus::Done(std::mem::take(resource)),
                        Err(error) => RequestStatus::Error(std::mem::take(error)),
                    }
                } else {
                    RequestStatus::Loading(promise)
                }
            }
            RequestStatus::Error(error) => RequestStatus::Error(error),
        };

        if let RequestStatus::Done(items) = &self.status {
            if self.loading_items.is_none() {
                if let Some(load_amount) = self.load_amount {
                    let to_load: Vec<_> = items
                        .iter()
                        .skip(self.stories.len())
                        .take(load_amount)
                        .map(|item_id| fetch_item(ctx.clone(), *item_id))
                        .collect();

                    self.loading_items = Some(to_load);
                    self.load_amount = None;
                } else if self.stories.is_empty() {
                    self.load_amount = Some(15);
                    ctx.request_repaint();
                }
            }
        }

        let old_page = self.page;

        let loaded_amount = self.loading_items.as_ref().map(|items| {
            items
                .iter()
                .filter(|promise| promise.ready().is_some())
                .count()
        });

        if let Some(promises) = &mut self.loading_items {
            if promises.len() == loaded_amount.unwrap() {
                let mut hn_stories = Vec::new();
                let mut error_message = String::new();
                for promise in &mut *promises {
                    let result = promise.ready().unwrap();
                    match result {
                        Ok(item) => {
                            hn_stories.push(item.clone());
                            if let Some(url) = &item.url {
                                self.favicons.insert(
                                    url.clone(),
                                    fetch_favicon::fetch_favicon(ctx.clone(), url.as_str()),
                                );
                            }
                        }
                        Err(error) => {
                            error_message.push_str(&error);
                            error_message.push('\n');
                        }
                    }
                }

                if error_message.is_empty() {
                    self.stories.append(&mut hn_stories);
                }

                self.loading_items = None;
            }
        }

        let mut go_back = false;

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding = Vec2::splat(5.0);

                self.y_icon.as_ref().unwrap().show(ui);

                ui.heading(RichText::new("Hacker News").strong());

                ui.add_space(10.0);

                ui.selectable_value(&mut self.page, Page::Top, "Top");
                ui.selectable_value(&mut self.page, Page::New, "New");
                ui.selectable_value(&mut self.page, Page::Show, "Show");
                ui.selectable_value(&mut self.page, Page::Ask, "Ask");
                ui.selectable_value(&mut self.page, Page::Jobs, "Jobs");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let size = ui.available_height() * 0.6;

                    match self.status {
                        RequestStatus::Done(_) | RequestStatus::Error(_) => {
                            if ui
                                .add_sized(
                                    [size, size],
                                    egui::Button::new(RichText::new("⟳").size(size * 0.6)),
                                )
                                .clicked()
                            {
                                self.refresh(&ctx);
                            }
                        }
                        RequestStatus::Loading(_) => {
                            ui.add(egui::Spinner::new().size(size));
                        }
                    }

                    let can_go_back = !matches!(self.status, RequestStatus::Loading(_))
                        && self.story_comments.is_some();

                    ui.add_enabled_ui(can_go_back, |ui| {
                        if ui
                            .add_sized(
                                [size, size],
                                egui::Button::new(RichText::new("⮨").size(size * 0.6)),
                            )
                            .clicked()
                        {
                            go_back = true;
                        }
                    });
                });
            });
        });

        if let Some(story_comments) = &self.story_comments.clone() {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_story(story_comments, ui, true, false);

                    ui.separator();

                    for comment_id in &story_comments.kids {
                        self.render_comment(*comment_id, ctx, ui);
                    }
                });
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| match &self.status {
                RequestStatus::Done(_) => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for story in &self.stories {
                            if self.render_story(story, ui, false, true) {
                                self.story_comments = Some(story.clone());
                            }

                            ui.separator();
                        }

                        ui.vertical_centered(|ui| {
                            let can_load_more = self.loading_items.is_none();

                            if ui
                                .add_enabled(can_load_more, egui::Button::new("Load More"))
                                .clicked()
                            {
                                self.load_amount = Some(15);
                            }
                        });
                    });
                }
                RequestStatus::Loading(_) => {
                    ui.label("Loading...");
                }
                RequestStatus::Error(error) => {
                    ui.vertical_centered(|ui| {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                        if ui.button("Retry").clicked() {
                            eprintln!("Retry");
                        }
                    });
                }
            });
        }

        let mut show_debug_window = self.show_debug_window;

        egui::Window::new("Debug")
            .open(&mut show_debug_window)
            .resizable(true)
            .scroll2([true, true])
            .default_width(500.0)
            .default_height(600.0)
            .show(ctx, |ui| {
                let mut debug_on_hover = ctx.style().debug.debug_on_hover;
                if ui.checkbox(&mut debug_on_hover, "Debug on hover").changed() {
                    let mut style = (*ctx.style()).clone();
                    style.debug.debug_on_hover = debug_on_hover;
                    ctx.set_style(style);
                }

                ui.checkbox(
                    &mut self.render_html,
                    "Render Html in story text and comments",
                );

                ui.separator();

                ui.label("Input Html text to render");
                ui.add(
                    egui::TextEdit::multiline(&mut self.text_input)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );

                self.render_html_text(&self.text_input, ui);
            });

        self.show_debug_window = show_debug_window;

        if go_back {
            self.story_comments = None;
        }

        if old_page != self.page {
            self.load_amount = None;
            self.stories.clear();
            self.loading_items = None;
            self.status = RequestStatus::Loading(fetch_page_stories(self.page, ctx.clone()));
            self.story_comments = None;
            ctx.request_repaint();
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let mut native_options = eframe::NativeOptions::default();
    native_options.initial_window_size = Some(Vec2::new(520., 960.));
    eframe::run_native(
        "Hacker News",
        native_options,
        Box::new(|cc| Box::new(Application::new(cc))),
    )
}
