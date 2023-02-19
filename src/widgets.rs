use eframe::{
    egui::{self, CollapsingHeader, RichText, TextFormat},
    epaint::{text::LayoutJob, FontId, Vec2},
};
use egui_extras::RetainedImage;

use crate::{comment_parser, human_format, HnItem, HnItemId};

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

pub(crate) fn html_text(text: &str, ui: &mut egui::Ui) {
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

pub(crate) fn story(
    story: &HnItem,
    ui: &mut egui::Ui,
    show_text: bool,
    can_open_comments: bool,
    render_html: bool,
    favicon: Option<&RetainedImage>,
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
            if let Some(icon) = favicon {
                let height = ui.available_height();
                icon.show_size(ui, Vec2::new(height, height));
            }

            ui.label(RichText::new(human_format::url(url)).monospace());
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
        ui.label(RichText::new(human_format::date_time(&story.time)).weak());
    });

    if show_text && story.text.len() > 0 {
        if render_html {
            html_text(&story.text, ui);
        } else {
            ui.label(&story.text);
        }
    }

    ui.horizontal(|ui| {
        if let Some(points_str) = human_format::points(story.score) {
            ui.label(&points_str);
            ui.label("•");
        }

        ui.add_enabled_ui(comment_link_enabled, |ui| {
            if ui
                .link(human_format::comment_count(story.descendants))
                .clicked()
            {
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

pub(crate) fn comment<F>(comment: &HnItem, ui: &mut egui::Ui, render_html: bool, draw_child: F)
where
    F: Fn(HnItemId, &mut egui::Ui),
{
    let mut text_layout = LayoutJob::default();
    if comment.by.len() > 0 {
        text_layout.append(
            &comment.by,
            0.0,
            TextFormat::simple(FontId::proportional(16.0), ui.visuals().strong_text_color()),
        );
        text_layout.append(
            "  •  ",
            0.0,
            TextFormat::simple(FontId::proportional(16.0), ui.visuals().weak_text_color()),
        );
    }
    text_layout.append(
        &human_format::date_time(&comment.time),
        0.0,
        TextFormat::simple(FontId::proportional(16.0), ui.visuals().weak_text_color()),
    );

    CollapsingHeader::new(text_layout)
        .id_source(comment.id)
        .default_open(true)
        .show(ui, |ui| {
            if comment.deleted {
                ui.label("[deleted]");
            } else {
                if render_html {
                    html_text(&comment.text, ui);
                } else {
                    ui.label(&comment.text);
                }
            }

            egui::Frame::none()
                .outer_margin(egui::style::Margin {
                    left: 20f32,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    for child in &comment.kids {
                        draw_child(*child, ui);
                    }
                });
        });
}
