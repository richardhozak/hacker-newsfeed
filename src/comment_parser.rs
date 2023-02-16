#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Item<'a> {
    Escape(char),
    Text(&'a str),
    NewLine,
    Link(Parser<'a>, Parser<'a>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct TextStyle {
    pub italic: bool,
    pub monospace: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Parser<'a> {
    s: &'a str,
    style: TextStyle,
    last_was_newline: bool,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            s: input,
            style: Default::default(),
            last_was_newline: false,
        }
    }

    pub fn to_string(&mut self) -> String {
        let mut string = String::new();
        for (item, _) in self {
            match item {
                Item::Escape(ch) => string.push(ch),
                Item::Text(text) => string.push_str(text),
                Item::NewLine => string.push('\n'),
                Item::Link(_, mut text) => string.push_str(&text.to_string()),
            }
        }

        string
    }

    fn return_item_or_next(&mut self, item: Item<'a>) -> Option<(Item<'a>, TextStyle)> {
        // Do not allow sending multiple new lines
        // and decorate item with current style.

        match (self.last_was_newline, item) {
            (true, Item::NewLine) => return self.next(),
            (_, item) => {
                self.last_was_newline = matches!(item, Item::NewLine);
                return Some((item, self.style));
            }
        }
    }
}

fn find_first_of(haystack: &str, needles: &[&str]) -> Option<usize> {
    let mut index = None;
    for needle in needles {
        if let Some(found_index) = haystack.find(needle) {
            if let Some(i) = index {
                if found_index < i {
                    index = Some(found_index);
                }
            } else {
                index = Some(found_index);
            }
        }
    }

    index
}

impl<'a> Iterator for Parser<'a> {
    type Item = (Item<'a>, TextStyle);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.s.is_empty() {
                return None;
            }

            if self.s.starts_with("<i>") {
                self.style.italic = true;
                self.s = &self.s["<i>".len()..];
                continue;
            }

            if self.s.starts_with("</i>") {
                self.style.italic = false;
                self.s = &self.s["</i>".len()..];
                continue;
            }

            if self.s.starts_with("<pre>") {
                // <pre> contains preformatted monospace text and is also block
                // element, meaning we should put it on its own line
                self.style.monospace = true;
                self.s = &self.s["<pre>".len()..];
                return self.return_item_or_next(Item::NewLine);
            }

            if self.s.starts_with("</pre>") {
                self.style.monospace = false;
                self.s = &self.s["</pre>".len()..];
                return self.return_item_or_next(Item::NewLine);
            }

            if self.s.starts_with("<code>") {
                self.style.monospace = true;
                self.s = &self.s["<code>".len()..];
                continue;
            }

            if self.s.starts_with("</code>") {
                self.style.monospace = false;
                self.s = &self.s["</code>".len()..];
                continue;
            }

            if self.s.starts_with("&#") {
                if let Some(index) = self.s.find(';') {
                    let mut num_str = &self.s[2..index];
                    if num_str.starts_with('x') {
                        num_str = &self.s[3..index];
                    }

                    if !num_str.is_empty() {
                        let mut num = 0;
                        for c in num_str.chars() {
                            if let Some(digit) = c.to_digit(16) {
                                num *= 0xF + 1;
                                num |= digit;
                            } else {
                                num = 0;
                                break;
                            }
                        }

                        if num != 0 {
                            if let Some(ch) = char::from_u32(num) {
                                self.s = &self.s[index + 1..];
                                return self.return_item_or_next(Item::Escape(ch));
                            }
                        }
                    }
                }
            }

            if self.s.starts_with("<p>") {
                self.s = &self.s[3..];
                return self.return_item_or_next(Item::NewLine);
            }

            if self.s.starts_with("&quot;") {
                self.s = &self.s["&quot;".len()..];
                return self.return_item_or_next(Item::Escape('"'));
            }

            if self.s.starts_with("&gt;") {
                self.s = &self.s["&gt;".len()..];
                return self.return_item_or_next(Item::Escape('>'));
            }

            if self.s.starts_with("<a href=\"") {
                let next_s = &self.s["<a href=\"".len()..];
                if let Some(end_url) = next_s.find('"') {
                    let url_str = &next_s[..end_url];

                    if let Some(begin_tag_end) = next_s.find('>') {
                        let next_s = &next_s[begin_tag_end + 1..];
                        if let Some(link_end) = next_s.find("</a>") {
                            let text_str = &next_s[..link_end];
                            self.s = &next_s[link_end + "</a>".len()..];
                            return self.return_item_or_next(Item::Link(
                                Parser::new(url_str),
                                Parser::new(text_str),
                            ));
                        }
                    }
                }
            }

            let remainder = &self.s[..find_first_of(
                self.s,
                &[
                    "&#",
                    "<p>",
                    "&gt;",
                    "&quot;",
                    "<a href=\"",
                    "<i>",
                    "</i>",
                    "<pre>",
                    "</pre>",
                    "<code>",
                    "</code>",
                ],
            )
            .unwrap_or(self.s.len())];
            if remainder.len() > 0 {
                self.s = &self.s[remainder.len()..];
                return self.return_item_or_next(Item::Text(remainder));
            }

            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_escape_with_x() {
        let input = "&#x27;";
        let mut parser = Parser::new(input);
        assert_eq!(
            parser.next(),
            Some((Item::Escape('\''), Default::default()))
        );
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn parses_single_escape_without_x() {
        let input = "&#27;";
        let mut parser = Parser::new(input);
        assert_eq!(
            parser.next(),
            Some((Item::Escape('\''), Default::default()))
        );
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn parses_text_only() {
        let input = " Hello world ";
        let mut parser = Parser::new(input);
        assert_eq!(
            parser.next(),
            Some((Item::Text(" Hello world "), Default::default()))
        );
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn parses_text_and_escape() {
        let input = "It&#x27;s a me Mario!";
        let mut parser = Parser::new(input);
        assert_eq!(parser.next(), Some((Item::Text("It"), Default::default())));
        assert_eq!(
            parser.next(),
            Some((Item::Escape('\''), Default::default()))
        );
        assert_eq!(
            parser.next(),
            Some((Item::Text("s a me Mario!"), Default::default()))
        );
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn parses_link() {
        let input = r#"<a href="https:&#x2F;&#x2F;www.vaultree.com&#x2F;how-it-works&#x2F;" rel="nofollow">https:&#x2F;&#x2F;www.vaultree.com&#x2F;how-it-works&#x2F;</a>"#;
        let expected = Item::Link(
            Parser::new("https:&#x2F;&#x2F;www.vaultree.com&#x2F;how-it-works&#x2F;"),
            Parser::new("https:&#x2F;&#x2F;www.vaultree.com&#x2F;how-it-works&#x2F;"),
        );
        let mut parser = Parser::new(input);
        assert_eq!(parser.next(), Some((expected, Default::default())));
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn has_italics() {
        let input = r#"<i>hello</i><p><i>there</i><p>general kenobi"#;
        let mut parser = Parser::new(input);

        assert_eq!(
            parser.next(),
            Some((
                Item::Text("hello"),
                TextStyle {
                    italic: true,
                    ..Default::default()
                }
            ))
        );

        assert_eq!(parser.next(), Some((Item::NewLine, Default::default())));

        assert_eq!(
            parser.next(),
            Some((
                Item::Text("there"),
                TextStyle {
                    italic: true,
                    ..Default::default()
                }
            ))
        );

        assert_eq!(parser.next(), Some((Item::NewLine, Default::default())));

        assert_eq!(
            parser.next(),
            Some((Item::Text("general kenobi"), Default::default()))
        );

        assert_eq!(parser.next(), None);
    }
}
