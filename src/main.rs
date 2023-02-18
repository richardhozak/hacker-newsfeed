#![allow(dead_code)]

use std::fmt::Display;

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
use tracing::warn;
use url::Url;

mod comment_parser;
mod fetch_favicon;

pub const DEBUG_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F12);
pub const REFRESH_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Hash, Default)]
struct HnItemId(usize);

impl Display for HnItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Deserialize, Clone)]
#[serde(default)]
struct HnItem {
    id: HnItemId,
    deleted: bool,
    r#type: String,
    by: String,
    #[serde(with = "time::serde::timestamp")]
    time: OffsetDateTime,
    text: String,
    dead: bool,
    parent: HnItemId,
    poll: HnItemId,
    kids: Vec<HnItemId>,
    url: Option<Url>,
    score: usize,
    title: String,
    parts: Vec<HnItemId>,
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
    display_stories: Vec<HnItemId>,

    display_comments_for_story: Option<HnItemId>,

    // items that are loaded or being loaded from api
    item_cache: HashMap<HnItemId, Promise<ehttp::Result<HnItem>>>,

    page_name: Page,    // what type of page/tab to display
    page_number: usize, // the story/article offset of given page to display
    page_size: usize,   // how many stories to display at once in page from page number offset
    page_status: RequestStatus,

    favicons: HashMap<Url, Promise<ehttp::Result<RetainedImage>>>,
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
fn fetch_page_stories(page: Page, ctx: egui::Context) -> Promise<ehttp::Result<Vec<HnItemId>>> {
    match page {
        Page::Top => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/topstories.json"),
        Page::New => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/newstories.json"),
        Page::Show => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/showstories.json"),
        Page::Ask => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/askstories.json"),
        Page::Jobs => fetch_url(ctx, "https://hacker-news.firebaseio.com/v0/jobstories.json"),
    }
}

fn fetch_item(ctx: egui::Context, item_id: HnItemId) -> Promise<ehttp::Result<HnItem>> {
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

        let page_status =
            RequestStatus::Loading(fetch_page_stories(Page::Top, cc.egui_ctx.clone()));

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
            page_status,
            page_size: 15,
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

    fn render_comment(&mut self, comment_id: HnItemId, ctx: &egui::Context, ui: &mut egui::Ui) {
        let promise = match self.item_cache.remove(&comment_id) {
            Some(promise) => promise,
            None => return,
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
        }

        self.item_cache.insert(comment_id, promise);
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.page_status = RequestStatus::Loading(fetch_page_stories(self.page_name, ctx.clone()));
    }

    fn load_comments(&mut self, item: &HnItem, ctx: &egui::Context) -> bool {
        let mut loaded = true;

        for &kid in &item.kids {
            let promise = match self.item_cache.remove(&kid) {
                Some(promise) => promise,
                None => fetch_item(ctx.clone(), kid),
            };

            if let Some(result) = promise.ready() {
                match result {
                    Ok(kid_item) => {
                        if !self.load_comments(kid_item, ctx) {
                            loaded = false;
                        }
                    }
                    Err(error) => warn!("cannot load comment: {}", error),
                }
            } else {
                loaded = false;
            }

            self.item_cache.insert(kid, promise);
        }

        loaded
    }

    fn load_more(&mut self, ctx: &egui::Context) {
        if let RequestStatus::Done(item_ids) = &self.page_status {
            self.display_stories.clear();

            for &id in item_ids
                .iter()
                .skip(self.page_number * self.page_size)
                .take(self.page_size)
            {
                self.display_stories.push(id);
                self.item_cache
                    .entry(id)
                    .or_insert_with(|| fetch_item(ctx.clone(), id));
            }

            self.page_number += 1;
        }
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
    Done(Vec<HnItemId>),
    Loading(Promise<ehttp::Result<Vec<HnItemId>>>),
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

        let mut just_loaded_page = false;

        self.page_status = match std::mem::take(&mut self.page_status) {
            RequestStatus::Done(items) => RequestStatus::Done(items),
            RequestStatus::Loading(mut promise) => {
                if let Some(result) = promise.ready_mut() {
                    match result {
                        Ok(resource) => {
                            just_loaded_page = true;
                            RequestStatus::Done(std::mem::take(resource))
                        }
                        Err(error) => RequestStatus::Error(std::mem::take(error)),
                    }
                } else {
                    RequestStatus::Loading(promise)
                }
            }
            RequestStatus::Error(error) => RequestStatus::Error(error),
        };

        if just_loaded_page {
            if self.page_number == 0 {
                self.load_more(ctx);
            }
        }

        let old_page = self.page_name;
        let mut go_back = false;

        let loading_any_items = self.item_cache.iter().any(|(_, p)| p.ready().is_none());
        let loading_stories = self
            .item_cache
            .iter()
            .any(|(id, p)| p.ready().is_none() && self.display_stories.contains(id));

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding = Vec2::splat(5.0);

                self.y_icon.as_ref().unwrap().show(ui);

                ui.heading(RichText::new("Hacker News").strong());

                ui.add_space(10.0);

                ui.selectable_value(&mut self.page_name, Page::Top, "Top");
                ui.selectable_value(&mut self.page_name, Page::New, "New");
                ui.selectable_value(&mut self.page_name, Page::Show, "Show");
                ui.selectable_value(&mut self.page_name, Page::Ask, "Ask");
                ui.selectable_value(&mut self.page_name, Page::Jobs, "Jobs");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let size = ui.available_height() * 0.6;

                    match (&self.page_status, loading_any_items) {
                        (RequestStatus::Done(_) | RequestStatus::Error(_), false) => {
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
                        _ => {
                            ui.add(egui::Spinner::new().size(size));
                        }
                    }

                    let can_go_back = !matches!(self.page_status, RequestStatus::Loading(_))
                        && self.display_comments_for_story.is_some();

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

        if let Some(story_id) = &self.display_comments_for_story.clone() {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(promise) = self.item_cache.remove(story_id) {
                        if let Some(result) = promise.ready() {
                            if let Ok(story) = result {
                                self.render_story(story, ui, true, false);

                                ui.separator();

                                if !self.load_comments(story, ctx) {
                                    ui.label("Loading...");
                                }

                                for comment_id in &story.kids {
                                    self.render_comment(*comment_id, ctx, ui);
                                }
                            }
                        }

                        self.item_cache.insert(*story_id, promise);
                    }
                });
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                match (&self.page_status, loading_stories) {
                    (RequestStatus::Done(_), false) => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for story_id in &self.display_stories {
                                if let Some(promise) = self.item_cache.get(story_id) {
                                    if let Some(result) = promise.ready() {
                                        if let Ok(story) = result {
                                            if self.render_story(story, ui, false, true) {
                                                self.display_comments_for_story = Some(story.id);
                                            }
                                        }
                                    }
                                }

                                ui.separator();
                            }

                            ui.vertical_centered(|ui| {
                                if ui
                                    .add_enabled(!loading_any_items, egui::Button::new("Load More"))
                                    .clicked()
                                {
                                    self.load_more(ctx);
                                }
                            });
                        });
                    }
                    (RequestStatus::Error(error), false) => {
                        ui.vertical_centered(|ui| {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                            if ui.button("Retry").clicked() {
                                eprintln!("Retry");
                            }
                        });
                    }
                    _ => {
                        ui.label("Loading...");
                    }
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
            self.display_comments_for_story = None;
        }

        if old_page != self.page_name {
            self.display_comments_for_story = None;
            self.display_stories.clear();
            self.page_status =
                RequestStatus::Loading(fetch_page_stories(self.page_name, ctx.clone()));
            self.page_number = 0;
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
