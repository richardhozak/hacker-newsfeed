#![allow(dead_code)]

use std::fmt::Display;

use eframe::{
    egui::{self, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText, TextStyle},
    epaint::{ahash::HashMap, Vec2},
    CreationContext,
};
use egui_extras::RetainedImage;
use poll_promise::Promise;
use serde::Deserialize;
use time::OffsetDateTime;
use tracing::warn;
use url::Url;

mod comment_parser;
mod fetch;
mod human_format;
mod widgets;

pub const DEBUG_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F12);
pub const REFRESH_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
pub const GO_BACK_FROM_COMMENTS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::NONE, Key::Backspace);
pub const GO_BACK: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowLeft);
pub const GO_NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowRight);
pub const TAB_TOP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::Num1);
pub const TAB_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::Num2);
pub const TAB_SHOW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::Num3);
pub const TAB_ASK: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::Num4);
pub const TAB_JOBS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::Num5);

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
    descendants: usize, // total comment count when type is story
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

#[derive(Default, Clone, Copy, Debug, PartialEq)]
enum Page {
    #[default]
    Top,
    New,
    Show,
    Ask,
    Jobs,
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

struct Application {
    display_comments_for_story: Option<HnItemId>,

    // items that are loaded or being loaded from api
    item_cache: HashMap<HnItemId, Promise<ehttp::Result<HnItem>>>,

    // page state
    page_name: Page,    // what type of page/tab to display
    page_number: usize, // the story/article offset of given page to display
    page_size: usize,   // how many stories to display at once in page from page number offset
    page_status: RequestStatus,

    // icons
    favicons: HashMap<Url, Promise<ehttp::Result<RetainedImage>>>,
    default_icon: RetainedImage,
    y_icon: RetainedImage,

    // debug
    render_html: bool,
    show_debug_window: bool,
    text_input: String,
}

impl Application {
    fn new(cc: &CreationContext) -> Self {
        configure_visuals(&cc.egui_ctx);
        configure_styles(&cc.egui_ctx);

        let default_icon = RetainedImage::from_image_bytes(
            "default_icon",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/default_icon.png"
            )),
        )
        .unwrap();

        let y_icon = RetainedImage::from_image_bytes(
            "y_icon",
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/y_icon.png")),
        )
        .unwrap();

        Self {
            display_comments_for_story: None,
            item_cache: Default::default(),
            page_name: Default::default(),
            page_number: 0,
            page_size: 15,
            page_status: RequestStatus::Loading(fetch::page_stories(
                Default::default(),
                cc.egui_ctx.clone(),
            )),
            default_icon: default_icon,
            y_icon: y_icon,
            render_html: true,
            favicons: Default::default(),
            show_debug_window: false,
            text_input: String::new(),
        }
    }

    fn render_html_text(&self, text: &str, ui: &mut egui::Ui) {
        if self.render_html {
            widgets::html_text(text, ui);
        } else {
            ui.label(text);
        }
    }

    fn load_missing_icons(&mut self, ctx: &egui::Context) {
        for (_, promise) in &self.item_cache {
            if let Some(result) = promise.ready() {
                if let Ok(item) = result {
                    if let Some(url) = &item.url {
                        if !self.favicons.contains_key(url) {
                            self.favicons
                                .insert(url.clone(), fetch::favicon(ctx.clone(), url.as_str()));
                        }
                    }
                }
            }
        }
    }

    fn get_favicon_or_default(&self, url: &Url) -> &RetainedImage {
        self.favicons
            .get(&url)
            .and_then(|promise| promise.ready())
            .and_then(|result| result.as_ref().ok())
            .unwrap_or(&self.default_icon)
    }

    fn render_story(
        &self,
        story: &HnItem,
        ui: &mut egui::Ui,
        show_text: bool,
        can_open_comments: bool,
    ) -> bool {
        return widgets::story(
            story,
            ui,
            show_text,
            can_open_comments,
            self.render_html,
            story
                .url
                .as_ref()
                .map(|url| self.get_favicon_or_default(url)),
        );
    }

    fn render_comment(&self, comment_id: HnItemId, ui: &mut egui::Ui) {
        let promise = match self.item_cache.get(&comment_id) {
            Some(promise) => promise,
            None => return,
        };

        if let Some(result) = promise.ready() {
            match result {
                Ok(comment) => widgets::comment(comment, ui, self.render_html, |child_id, ui| {
                    self.render_comment(child_id, ui)
                }),
                Err(error) => {
                    ui.label(format!("Error: {}", error));
                }
            };
        }
    }

    fn remove_item_with_kids(&mut self, item_id: HnItemId) {
        if let Some(promise) = self.item_cache.remove(&item_id) {
            if let Ok(result) = promise.try_take() {
                if let Ok(item) = result {
                    for kid_id in &item.kids {
                        self.remove_item_with_kids(*kid_id);
                    }
                }
            }
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if let Some(story_id) = self.display_comments_for_story {
            self.remove_item_with_kids(story_id);
        } else {
            self.item_cache.clear();
            self.page_status =
                RequestStatus::Loading(fetch::page_stories(self.page_name, ctx.clone()));
        }
    }

    fn load_comments(&mut self, item: &HnItem, ctx: &egui::Context) -> bool {
        let mut loaded = true;

        for &kid in &item.kids {
            let promise = match self.item_cache.remove(&kid) {
                Some(promise) => promise,
                None => fetch::hn_item(ctx.clone(), kid),
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

    fn load_missing_comments_for_opened_story(&mut self, ctx: &egui::Context) {
        if let Some(story_id) = self.display_comments_for_story {
            if let Some(promise) = self.item_cache.remove(&story_id) {
                if let Some(result) = promise.ready() {
                    if let Ok(story) = result {
                        self.load_comments(&story, ctx);
                    }
                }

                self.item_cache.insert(story_id, promise);
            }
        }
    }

    fn load_missing_page_stories(&mut self, ctx: &egui::Context) {
        if let RequestStatus::Done(item_ids) = &self.page_status {
            for &id in self.displayed_page_stories(item_ids) {
                self.item_cache
                    .entry(id)
                    .or_insert_with(|| fetch::hn_item(ctx.clone(), id));
            }
        }
    }

    fn displayed_page_stories<'a>(
        &self,
        item_ids: &'a Vec<HnItemId>,
    ) -> impl Iterator<Item = &'a HnItemId> {
        item_ids
            .iter()
            .skip(self.page_number * self.page_size)
            .take(self.page_size)
    }

    fn get_item(&self, item_id: &HnItemId) -> Option<&HnItem> {
        self.item_cache
            .get(&item_id)
            .and_then(|promise| promise.ready())
            .and_then(|result| result.as_ref().ok())
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
        let mut go_back = false;

        if ctx.input_mut(|i| i.consume_shortcut(&DEBUG_SHORTCUT)) {
            self.show_debug_window = !self.show_debug_window;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&REFRESH_SHORTCUT)) {
            self.refresh(&ctx);
        }

        if ctx.input_mut(|i| i.consume_shortcut(&GO_BACK)) {
            go_back = true;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&GO_BACK_FROM_COMMENTS)) {
            if self.display_comments_for_story.is_some() {
                go_back = true;
            }
        }

        self.page_status = match std::mem::take(&mut self.page_status) {
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

        self.load_missing_page_stories(ctx);
        self.load_missing_icons(ctx);
        self.load_missing_comments_for_opened_story(ctx);

        let loading = matches!(self.page_status, RequestStatus::Loading(_))
            || self.item_cache.iter().any(|(_, p)| p.ready().is_none());
        let loading_stories = if let RequestStatus::Done(item_ids) = &self.page_status {
            self.displayed_page_stories(item_ids).any(|id| {
                self.item_cache
                    .get(id)
                    .map_or(false, |p| p.ready().is_none())
            })
        } else {
            false
        };

        let old_page = self.page_name;

        if ctx.input_mut(|i| i.consume_shortcut(&TAB_TOP)) {
            self.page_name = Page::Top;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&TAB_NEW)) {
            self.page_name = Page::New;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&TAB_SHOW)) {
            self.page_name = Page::Show;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&TAB_ASK)) {
            self.page_name = Page::Ask;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&TAB_JOBS)) {
            self.page_name = Page::Jobs;
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                self.y_icon.show(ui);

                ui.heading(RichText::new("Hacker News").strong());

                ui.add_space(10.0);

                ui.selectable_value(&mut self.page_name, Page::Top, "Top");
                ui.selectable_value(&mut self.page_name, Page::New, "New");
                ui.selectable_value(&mut self.page_name, Page::Show, "Show");
                ui.selectable_value(&mut self.page_name, Page::Ask, "Ask");
                ui.selectable_value(&mut self.page_name, Page::Jobs, "Jobs");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let size = ui.available_height() * 0.6;

                    match (&self.page_status, loading) {
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

                    let can_go_back =
                        self.display_comments_for_story.is_some() || self.page_number > 0;

                    let text = if self.display_comments_for_story.is_some() {
                        "↩" // "leftwards arrow with hook" - for going back to page from comment section
                    } else {
                        "⮨" // "black curved downwards and leftwards arrow" - for going back a page
                    };

                    ui.add_enabled_ui(can_go_back, |ui| {
                        if ui
                            .add_sized(
                                [size, size],
                                egui::Button::new(RichText::new(text).size(size * 0.6)),
                            )
                            .clicked()
                        {
                            go_back = true;
                        }
                    });
                });
            });
        });

        egui::TopBottomPanel::bottom("footer")
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if loading {
                        ui.label("Loading...");
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.hyperlink_to(
                            "\u{e624} Hacker Newsfeed on GitHub",
                            "https://www.github.com",
                        );
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(story_id) = self.display_comments_for_story {
                    if let Some(story) = self.get_item(&story_id) {
                        self.render_story(story, ui, true, false);

                        ui.separator();

                        for comment_id in &story.kids {
                            self.render_comment(*comment_id, ui);
                        }
                    }
                } else {
                    let error = match (&self.page_status, loading_stories) {
                        (RequestStatus::Done(story_items), false) => {
                            for story_id in self.displayed_page_stories(story_items) {
                                if let Some(story) = self.get_item(story_id) {
                                    if self.render_story(story, ui, false, true) {
                                        self.display_comments_for_story = Some(story.id);
                                    }

                                    ui.separator();
                                }
                            }

                            if ctx.input_mut(|i| i.consume_shortcut(&GO_NEXT)) {
                                if !loading {
                                    self.page_number += 1;
                                }
                            }

                            ui.vertical_centered(|ui| {
                                if ui
                                    .add_enabled(!loading, egui::Button::new("Load More"))
                                    .clicked()
                                {
                                    self.page_number += 1;
                                }
                            });

                            None
                        }
                        (RequestStatus::Error(error), false) => Some(error.to_string()),
                        _ => None,
                    };

                    if let Some(error) = error {
                        ui.vertical_centered(|ui| {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                            if ui.button("Retry").clicked() {
                                self.refresh(ctx);
                            }
                        });
                    }
                }
            });
        });

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
            if self.display_comments_for_story.is_some() {
                self.display_comments_for_story = None;
            } else if self.page_number > 0 {
                self.page_number -= 1;
            }
        }

        if old_page != self.page_name {
            self.display_comments_for_story = None;
            self.page_status =
                RequestStatus::Loading(fetch::page_stories(self.page_name, ctx.clone()));
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
        "Hacker Newsfeed",
        native_options,
        Box::new(|cc| Box::new(Application::new(cc))),
    )
}
