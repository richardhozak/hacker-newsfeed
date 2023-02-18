use time::OffsetDateTime;
use url::Url;

pub(crate) fn date_time(date_time: &OffsetDateTime) -> String {
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

pub(crate) fn points(points: usize) -> Option<String> {
    match points {
        0 => None,
        1 => Some("1 point".to_string()),
        n => Some(format!("{} points", n)),
    }
}

pub(crate) fn url(url: &Url) -> String {
    url.host_str()
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| url.to_string())
}

pub(crate) fn comment_count(count: usize) -> String {
    match count {
        0 => "No comments".to_string(),
        1 => "1 comment".to_string(),
        n => format!("{} comments", n),
    }
}
