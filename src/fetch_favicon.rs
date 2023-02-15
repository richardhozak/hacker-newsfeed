use eframe::egui;
use egui_extras::RetainedImage;
use poll_promise::Promise;
use scraper::{Html, Selector};
use tracing::warn;
use url::Url;

pub(crate) fn fetch_favicon(
    ctx: egui::Context,
    url: &str,
) -> Promise<ehttp::Result<RetainedImage>> {
    // 1. try to fetch base url + /favicon.ico
    // 2. if that fails download the web page and check head for
    //   1. link rel shortcut icon href
    //   2. link rel icon href
    //
    // href can also be relative or absolute

    use poll_promise::Sender;

    fn fetch_favicon_or_else<T>(
        ctx: egui::Context,
        url: &str,
        sender: Sender<Result<RetainedImage, String>>,
        or_else: T,
    ) where
        T: FnOnce(egui::Context, &str, Sender<Result<RetainedImage, String>>) + Send + 'static,
    {
        let original_url = url.to_string();
        let request = ehttp::Request::get(url);
        ehttp::fetch(request, move |response| {
            if let Ok(response) = response {
                let content_type = response.content_type().unwrap_or_default();
                let image_result = if content_type.starts_with("image/svg") {
                    RetainedImage::from_svg_bytes(&response.url, &response.bytes)
                } else if content_type.starts_with("image/") {
                    RetainedImage::from_image_bytes(&response.url, &response.bytes)
                } else {
                    Err("Invalid content type".to_string())
                };

                match image_result {
                    Ok(image) => {
                        ctx.request_repaint(); // wake up UI thread, we have icon to re-render
                        sender.send(Ok(image));
                        return;
                    }
                    Err(error) => {
                        warn!(
                            "Could not read image: {} (content-type {}) from url {}",
                            error, content_type, response.url
                        );
                    }
                }
            }

            or_else(ctx, &original_url, sender);
        });
    }

    fn fetch_favicon_from_html(
        ctx: egui::Context,
        url: &str,
        sender: Sender<Result<RetainedImage, String>>,
    ) {
        let request = ehttp::Request::get(url);
        ehttp::fetch(request, move |response| match response {
            Ok(response) => {
                if let Some(text) = response.text() {
                    let html = Html::parse_document(text);
                    let selector = Selector::parse("link[rel~='icon']").unwrap();

                    if let Some(element) = html.select(&selector).next() {
                        if let Some(href) = element.value().attr("href") {
                            if let Some(url) = parse_favicon_url_from_base(&response.url, href) {
                                fetch_favicon_or_else(ctx, url.as_str(), sender, |_, _, sender| {
                                    sender.send(Err("Cannot fetch favicon".to_string()));
                                });
                                return;
                            };
                            sender.send(Err(format!(
                                "cannot resolve favicon href {} from {}",
                                href, response.url
                            )));
                            return;
                        }
                    }
                }

                sender.send(Err("Cannot fetch favicon".to_string()));
            }
            Err(error) => {
                sender.send(Err(error));
            }
        });
    }

    let (sender, promise) = Promise::new();

    if let Some(favicon_url) = get_favicon_url(&url) {
        fetch_favicon_or_else(ctx, &favicon_url, sender, fetch_favicon_from_html);
    } else {
        fetch_favicon_from_html(ctx, url, sender);
    }

    promise
}

fn get_favicon_url(url: &str) -> Option<String> {
    if let Ok(mut url) = Url::parse(url) {
        url.set_query(None);
        url.set_fragment(None);
        url.set_path("favicon.ico");

        match url.scheme() {
            "http" | "https" => Some(url.to_string()),
            _ => None,
        }
    } else {
        None
    }
}

/// `base_url` the url we want to resolve favicon against
/// `href` is path to favicon that can be either absolute or relative against `url`
fn parse_favicon_url_from_base(base_url: &str, href: &str) -> Option<Url> {
    if let Ok(favicon_url) = Url::parse(href) {
        return Some(favicon_url);
    }

    let mut base_url = base_url.to_string();
    if base_url.ends_with("/") {
        base_url.push_str("index.html");
    }

    if let Ok(base_url) = Url::parse(&base_url) {
        return Url::options().base_url(Some(&base_url)).parse(&href).ok();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_favicon_hrefs() {
        let items: &[(&str, &str, Url)] = &[
            ("https://maximiliangolla.com/blog/2022-10-wol-plex-server/", "../../ico/favicon.ico", "https://maximiliangolla.com/ico/favicon.ico".parse().unwrap()),
            ("https://medium.com/@pravse/the-maze-is-in-the-mouse-980c57cfd61a", "https://miro.medium.com/1*m-R_BkNf1Qjr1YbyOIJY2w.png", "https://miro.medium.com/1*m-R_BkNf1Qjr1YbyOIJY2w.png".parse().unwrap()),
            ("https://rwmj.wordpress.com/2023/02/14/frame-pointers-vs-dwarf-my-verdict/", "https://s1.wp.com/i/favicon.ico", "https://s1.wp.com/i/favicon.ico".parse().unwrap()),
            ("https://ounapuu.ee/posts/2023/02/15/shrinkflation/", "https://ounapuu.ee/media/favicon.png", "https://ounapuu.ee/media/favicon.png".parse().unwrap()),
            ("https://www.bbc.com/future/article/20230208-the-tech-revealing-hidden-doodles-in-old-books-and-objects", "https://static-web-assets.gnl-common.bbcverticals.com/features/pwa/20230202-144939-a559125f5f495a867aaa5fa4d720d402dce4f7a4/future/favicon-32x32.png", "https://static-web-assets.gnl-common.bbcverticals.com/features/pwa/20230202-144939-a559125f5f495a867aaa5fa4d720d402dce4f7a4/future/favicon-32x32.png".parse().unwrap()),
            ("https://matplotlib.org/stable/users/prev_whats_new/whats_new_3.7.0.html", "../../_static/favicon.ico", "https://matplotlib.org/stable/_static/favicon.ico".parse().unwrap()),
            ("https://github.com/dfloer/SC2k-docs", "https://github.githubassets.com/favicons/favicon.png", "https://github.githubassets.com/favicons/favicon.png".parse().unwrap()),
            ("https://brr.fyi/posts/last-flight-out", "/favicon-32x32.png", "https://brr.fyi/favicon-32x32.png".parse().unwrap()),
            ("https://mainichi.jp/english/articles/20230214/p2g/00m/0bu/043000c", "https://cdn.mainichi.jp/vol1/images/icon/english/favicon.ico", "https://cdn.mainichi.jp/vol1/images/icon/english/favicon.ico".parse().unwrap()),
            ("https://twitter.com/DrJimFan/status/1625538305889820673", "//abs.twimg.com/favicons/twitter.2.ico", "https://abs.twimg.com/favicons/twitter.2.ico".parse().unwrap()),
            ("https://careergpt.ai/", "/favicon.ico", "https://careergpt.ai/favicon.ico".parse().unwrap()),
            ("https://theflaw.org/articles/the-price-of-a-harvard-lawyer/", "https://theflaw.org/wp-content/themes/sink_theflaw/images/favicon.ico?v=1676468817", "https://theflaw.org/wp-content/themes/sink_theflaw/images/favicon.ico?v=1676468817".parse().unwrap()),
        ];

        for (base_url, href, favicon_url) in items {
            let result = parse_favicon_url_from_base(base_url, href);
            assert_eq!(result.as_ref(), Some(favicon_url));
        }
    }
}
