#![allow(dead_code)]

use eframe::{
    egui::{self, CollapsingHeader, Color32, FontId, RichText, TextFormat, TextStyle},
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

#[derive(Clone)]
struct Story {
    url: Option<Url>,
    title: String,
    author: String,
    created: OffsetDateTime,
    points: usize,
    comments: usize,

    item: HnItem,
}

impl Story {
    fn from_hn_item(item: &HnItem) -> Self {
        Self {
            url: Url::parse(&item.url).ok(),
            title: item.title.clone(),
            author: item.by.clone(),
            created: OffsetDateTime::from_unix_timestamp(item.time)
                .unwrap_or_else(|_| OffsetDateTime::now_utc()),
            points: item.score,
            comments: item.descendants,
            item: item.clone(),
        }
    }
}

#[derive(Default, Deserialize, Clone)]
#[serde(default)]
struct HnItem {
    id: usize,
    deleted: bool,
    r#type: String,
    by: String,
    time: i64,
    text: String,
    dead: bool,
    parent: usize,
    poll: usize,
    kids: Vec<usize>,
    url: String,
    score: usize,
    title: String,
    parts: Vec<usize>,
    descendants: usize,
}

#[derive(Default)]
struct Application {
    stories: Vec<Story>,
    loading_items: Option<Vec<Promise<ehttp::Result<HnItem>>>>,
    status: RequestStatus,
    load_amount: Option<usize>,
    story_comments: Option<Story>,
    items: HashMap<usize, Promise<ehttp::Result<HnItem>>>,
    favicons: HashMap<String, Promise<ehttp::Result<RetainedImage>>>,
    page: Page,
    default_icon: Option<RetainedImage>,
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

fn configure_text_styles(ctx: &egui::Context) {
    use egui::FontFamily::{Monospace, Proportional};

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(8.0, Proportional)),
        (TextStyle::Body, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, Monospace)),
        (TextStyle::Button, FontId::new(12.0, Proportional)),
        (TextStyle::Heading, FontId::new(22.0, Proportional)),
    ]
    .into();
    ctx.set_style(style);
}

fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();

    // #f6f6ef

    // visuals.window_fill = Color32::BROWN;
    // visuals.widgets.noninteractive.bg_fill = Color32::BROWN;

    // the background of central panel
    visuals.panel_fill = Color32::from_rgb(0xf6, 0xf6, 0xef);

    // the background of scrollbar behind the handle
    visuals.extreme_bg_color = Color32::from_rgb(0xf6, 0xf6, 0xef);

    ctx.set_visuals(visuals);
}

// #f6f6ef

fn render_html_text(text: &str, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        let parser = comment_parser::Parser::new(text);
        for item in parser {
            match item {
                comment_parser::Item::Escape(c) => {
                    ui.label(c.to_string());
                }
                comment_parser::Item::Text(text) => {
                    ui.label(text);
                }
                comment_parser::Item::NewLine => {
                    ui.label("\n");
                }
                comment_parser::Item::Link(mut url, mut text) => {
                    let url = url.to_string();
                    let text = text.to_string();
                    ui.hyperlink_to(text, url);
                }
            }
        }
    });
}

impl Application {
    fn new(cc: &CreationContext) -> Self {
        configure_visuals(&cc.egui_ctx);
        configure_text_styles(&cc.egui_ctx);

        let status = RequestStatus::Loading(fetch_page_stories(Page::Top, cc.egui_ctx.clone()));

        let default_icon = RetainedImage::from_image_bytes(
            "default_icon",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/default_icon.png" // https://icons8.com/icon/NyuxPErq0tu2/globe-africa
            )),
        )
        .unwrap();

        Self {
            status,
            default_icon: Some(default_icon),
            ..Default::default()
        }
    }

    fn render_story(
        &self,
        story: &Story,
        ui: &mut egui::Ui,
        show_text: bool,
        interactive: bool,
    ) -> bool {
        let mut open_comments = false;

        if let Some(url) = &story.url {
            ui.horizontal(|ui| {
                if let Some(promise) = self.favicons.get(&story.item.url) {
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
        let response = if interactive {
            ui.scope(|ui| {
                ui.visuals_mut().hyperlink_color = ui.visuals().widgets.active.fg_stroke.color;
                ui.link(title_text)
            })
            .inner
        } else {
            ui.label(title_text)
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new(&story.author).strong());
            ui.label("•");
            ui.label(RichText::new(format_date_time(&story.created)).weak());
        });

        if show_text && story.item.text.len() > 0 {
            render_html_text(&story.item.text, ui);
        }

        ui.horizontal(|ui| {
            if let Some(points_str) = format_points(story.points) {
                ui.label(&points_str);
                ui.label("•");
            }

            ui.add_enabled_ui(story.comments > 0 && interactive, |ui| {
                if ui.link(format_comments(story.comments)).clicked() {
                    open_comments = true;
                }
            });
        });

        if response.clicked() {
            if let Some(url) = &story.url {
                if let Err(err) = webbrowser::open(url.as_str()) {
                    warn!("Could not open webbrowser {}", err);
                }
            } else {
                open_comments = true;
            }
        }

        open_comments
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
                        &format_unix_timestamp(comment.time),
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
                                render_html_text(&comment.text, ui);
                            }
                        });

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

fn format_unix_timestamp(timestamp: i64) -> String {
    let date_time =
        OffsetDateTime::from_unix_timestamp(timestamp).unwrap_or(OffsetDateTime::now_utc());

    format_date_time(&date_time)
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
                            hn_stories.push(Story::from_hn_item(item));
                            if !item.url.is_empty() {
                                self.favicons.insert(
                                    item.url.to_string(),
                                    fetch_favicon::fetch_favicon(ctx.clone(), &item.url),
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

        egui::TopBottomPanel::top("top_menu_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Hacker News").strong());

                match self.status {
                    RequestStatus::Done(_) | RequestStatus::Error(_) => {
                        if ui.button("Refresh").clicked() {
                            self.status =
                                RequestStatus::Loading(fetch_page_stories(self.page, ctx.clone()));
                        }
                    }
                    RequestStatus::Loading(_) => {
                        ui.spinner();
                    }
                }

                if let Some(items) = &self.loading_items {
                    ui.label(format!(
                        "loaded {}/{}",
                        loaded_amount.unwrap_or(0),
                        items.len()
                    ));
                }
            });

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, Page::Top, "[Top]");
                ui.selectable_value(&mut self.page, Page::New, "[New]");
                ui.selectable_value(&mut self.page, Page::Show, "[Show]");
                ui.selectable_value(&mut self.page, Page::Ask, "[Ask]");
                ui.selectable_value(&mut self.page, Page::Jobs, "[Jobs]");
            });
        });

        let mut go_back = false;

        if let Some(story_comments) = &self.story_comments.clone() {
            egui::CentralPanel::default().show(ctx, |ui| {
                if ui.button("Back").clicked() {
                    go_back = true;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_story(story_comments, ui, true, false);

                    ui.separator();

                    for comment_id in &story_comments.item.kids {
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

                        match self.load_amount {
                            Some(_) => {
                                ui.spinner();
                            }
                            None => {
                                if ui.button("Load more").clicked() {
                                    self.load_amount = Some(15);
                                }
                            }
                        }
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

fn main() {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let mut native_options = eframe::NativeOptions::default();
    native_options.initial_window_size = Some(Vec2::new(520., 960.));
    eframe::run_native(
        "Hacker News",
        native_options,
        Box::new(|cc| Box::new(Application::new(cc))),
    );
}
