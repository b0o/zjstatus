use cached::{LruCache, macros::cached};
use lazy_static::lazy_static;
use std::{collections::BTreeMap, sync::Arc};

use anstyle::{Ansi256Color, AnsiColor, Color, RgbColor, Style};
use regex::Regex;
use zellij_tile::prelude::bail;

use crate::{
    config::{UpdateEventMask, ZellijState, event_mask_from_widget_name},
    widgets::widget::Widget,
};

lazy_static! {
    static ref WIDGET_REGEX: Regex = Regex::new("(\\{[a-z_0-9]+\\})").unwrap();
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormattedPart {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub us: Option<Color>,
    pub effects: anstyle::Effects,
    pub bold: bool,
    pub italic: bool,
    pub underscore: bool,
    pub reverse: bool,
    pub blink: bool,
    pub hidden: bool,
    pub dimmed: bool,
    pub strikethrough: bool,
    pub double_underscore: bool,
    pub curly_underscore: bool,
    pub dotted_underscore: bool,
    pub dashed_underscore: bool,
    pub content: String,
    pub cache_mask: u8,
    pub cached_content: String,
    pub cache: BTreeMap<String, String>,
}

#[cached(
    ty = "LruCache<String, FormattedPart>",
    create = "{ LruCache::builder().max_size(100).build().unwrap() }",
    convert = r#"{ (format.to_owned()) }"#
)]
pub fn formatted_part_from_string_cached(
    format: &str,
    config: &BTreeMap<String, String>,
) -> FormattedPart {
    FormattedPart::from_format_string(format, config)
}

#[cached(
    ty = "LruCache<String, Vec<FormattedPart>>",
    create = "{ LruCache::builder().max_size(100).build().unwrap() }",
    convert = r#"{ (config_string.to_owned()) }"#
)]
pub fn formatted_parts_from_string_cached(
    config_string: &str,
    config: &BTreeMap<String, String>,
) -> Vec<FormattedPart> {
    FormattedPart::multiple_from_format_string(config_string, config)
}

impl FormattedPart {
    pub fn multiple_from_format_string(
        config_string: &str,
        config: &BTreeMap<String, String>,
    ) -> Vec<Self> {
        config_string
            .split("#[")
            .map(|s| FormattedPart::from_format_string(s, config))
            .collect()
    }

    pub fn from_format_string(format: &str, config: &BTreeMap<String, String>) -> Self {
        let format = match format.starts_with("#[") {
            true => format.strip_prefix("#[").unwrap(),
            false => format,
        };

        let mut result = FormattedPart {
            cache_mask: cache_mask_from_content(format),
            ..Default::default()
        };

        let mut format_content_split = format.split(']').collect::<Vec<&str>>();

        if format_content_split.len() == 1 {
            format.clone_into(&mut result.content);

            return result;
        }

        let parts = format_content_split[0].split(',');

        format_content_split.remove(0);
        result.content = format_content_split.join("]");

        for part in parts {
            if part.starts_with("fg=") {
                result.fg = parse_color(part.strip_prefix("fg=").unwrap(), config);
            }

            if part.starts_with("bg=") {
                result.bg = parse_color(part.strip_prefix("bg=").unwrap(), config);
            }

            if part.starts_with("us=") {
                result.us = parse_color(part.strip_prefix("us=").unwrap(), config);
            }

            if part.eq("reverse") {
                result.reverse = true;
            }

            result.parse_and_set_effect(part);
        }

        result
    }

    fn parse_and_set_effect(&mut self, part: &str) {
        match part {
            "bold" => {
                self.effects |= anstyle::Effects::BOLD;
            }
            "italic" | "italics" => {
                self.effects |= anstyle::Effects::ITALIC;
            }
            "underscore" => {
                self.effects |= anstyle::Effects::UNDERLINE;
            }
            "blink" => {
                self.effects |= anstyle::Effects::BLINK;
            }
            "hidden" => {
                self.effects |= anstyle::Effects::HIDDEN;
            }
            "dim" => {
                self.effects |= anstyle::Effects::DIMMED;
            }
            "strikethrough" => {
                self.effects |= anstyle::Effects::STRIKETHROUGH;
            }
            "double-underscore" => {
                self.effects |= anstyle::Effects::DOUBLE_UNDERLINE;
            }
            "curly-underscore" => {
                self.effects |= anstyle::Effects::CURLY_UNDERLINE;
            }
            "dotted-underscore" => {
                self.effects |= anstyle::Effects::DOTTED_UNDERLINE;
            }
            "dashed-underscore" => {
                self.effects |= anstyle::Effects::DASHED_UNDERLINE;
            }
            "reverse" => {
                self.effects |= anstyle::Effects::INVERT;
            }
            _ => {}
        }
    }

    pub fn format_string(&self, text: &str) -> String {
        let mut style = Style::new();

        style = style.fg_color(self.fg);
        style = style.bg_color(self.bg);
        style = style.underline_color(self.us);
        style = style.effects(self.effects);

        format!(
            "{}{}{}{}",
            style.render_reset(),
            style.render(),
            text,
            style.render_reset()
        )
    }

    #[tracing::instrument(skip_all)]
    pub fn format_string_with_widgets(
        &mut self,
        widgets: &BTreeMap<String, Arc<dyn Widget>>,
        state: &ZellijState,
    ) -> String {
        let skip_cache = self.cache_mask & UpdateEventMask::Always as u8 != 0;

        if !skip_cache && self.cache_mask & state.cache_mask == 0 && !self.cache.is_empty() {
            tracing::debug!(msg = "hit", typ = "format_string", format = self.content);
            return self.cached_content.to_owned();
        }
        tracing::debug!(msg = "miss", typ = "format_string", format = self.content);

        let mut output = String::with_capacity(self.content.len());
        let mut end = 0;

        // Substitute only original template spans, never text returned by a widget.
        for widget in WIDGET_REGEX.find_iter(&self.content) {
            output.push_str(&self.content[end..widget.start()]);
            end = widget.end();
            let match_name = widget.as_str();
            let widget_key = match_name.trim_matches(|c| c == '{' || c == '}');
            let mut widget_key_name = widget_key;

            if widget_key.starts_with("command_") {
                widget_key_name = "command";
            }

            if widget_key.starts_with("pipe_") {
                widget_key_name = "pipe";
            }

            let widget_mask = event_mask_from_widget_name(widget_key_name);
            let skip_widget_cache = widget_mask & UpdateEventMask::Always as u8 != 0;
            if !skip_widget_cache
                && widget_mask & state.cache_mask == 0
                && let Some(res) = self.cache.get(widget_key)
            {
                tracing::debug!(msg = "hit", typ = "widget", widget = widget_key);
                output.push_str(res);
                continue;
            }

            tracing::debug!(
                msg = "miss",
                typ = "widget",
                widget = widget_key,
                mask = widget_mask & state.cache_mask,
                skip_cache = skip_cache,
            );

            let result = match widgets.get(widget_key_name) {
                Some(widget) => widget.process(widget_key, state),
                None => "Use of uninitialized widget".to_owned(),
            };

            self.cache.insert(widget_key.to_owned(), result.to_owned());

            output.push_str(&result);
        }
        output.push_str(&self.content[end..]);

        let res = self.format_string(&output);
        self.cached_content.clone_from(&res);

        res
    }
}

impl Default for FormattedPart {
    fn default() -> Self {
        Self {
            fg: None,
            bg: None,
            us: None,
            effects: anstyle::Effects::new(),
            bold: false,
            italic: false,
            underscore: false,
            reverse: false,
            blink: false,
            hidden: false,
            dimmed: false,
            strikethrough: false,
            double_underscore: false,
            curly_underscore: false,
            dotted_underscore: false,
            dashed_underscore: false,
            content: "".to_owned(),
            cache_mask: 0,
            cached_content: "".to_owned(),
            cache: BTreeMap::new(),
        }
    }
}

fn cache_mask_from_content(content: &str) -> u8 {
    let mut output = 0;
    for widget in WIDGET_REGEX.captures_iter(content) {
        let match_name = widget.get(0).unwrap().as_str();
        let widget_key = match_name.trim_matches(|c| c == '{' || c == '}');
        let mut widget_key_name = widget_key;

        if widget_key.starts_with("command_") {
            widget_key_name = "command";
        }

        if widget_key.starts_with("pipe_") {
            widget_key_name = "pipe";
        }

        output |= event_mask_from_widget_name(widget_key_name);
    }
    output
}

fn hex_to_rgb(s: &str) -> anyhow::Result<Vec<u8>> {
    if s.len() != 6 {
        bail!("wrong hex color length");
    }

    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(anyhow::Error::from))
        .collect()
}

#[cached(
    ty = "LruCache<String, Color>",
    create = "{ LruCache::builder().max_size(100).build().unwrap() }",
    convert = r#"{ (color.to_owned()) }"#
)]
fn parse_color(color: &str, config: &BTreeMap<String, String>) -> Option<Color> {
    let mut color = color;
    if color.starts_with('$') {
        let alias_name = color.strip_prefix('$').unwrap();

        color = config.get(&format!("color_{alias_name}"))?;
    }

    if color.starts_with('#') {
        let rgb = match hex_to_rgb(color.strip_prefix('#').unwrap()) {
            Ok(rgb) => rgb,
            Err(_) => return None,
        };

        if rgb.len() != 3 {
            return None;
        }

        return Some(
            RgbColor(
                *rgb.first().unwrap(),
                *rgb.get(1).unwrap(),
                *rgb.get(2).unwrap(),
            )
            .into(),
        );
    }

    if let Some(color) = color_by_name(color) {
        return Some(color.into());
    }

    if color.starts_with("colour") {
        color = color.strip_prefix("colour").unwrap();
    }

    if let Ok(result) = color.parse::<u8>() {
        return Some(Ansi256Color(result).into());
    }

    None
}

fn color_by_name(color: &str) -> Option<AnsiColor> {
    match color {
        "black" => Some(AnsiColor::Black),
        "red" => Some(AnsiColor::Red),
        "green" => Some(AnsiColor::Green),
        "yellow" => Some(AnsiColor::Yellow),
        "blue" => Some(AnsiColor::Blue),
        "magenta" => Some(AnsiColor::Magenta),
        "cyan" => Some(AnsiColor::Cyan),
        "white" => Some(AnsiColor::White),
        "bright_black" => Some(AnsiColor::BrightBlack),
        "bright_red" => Some(AnsiColor::BrightRed),
        "bright_green" => Some(AnsiColor::BrightGreen),
        "bright_yellow" => Some(AnsiColor::BrightYellow),
        "bright_blue" => Some(AnsiColor::BrightBlue),
        "bright_magenta" => Some(AnsiColor::BrightMagenta),
        "bright_cyan" => Some(AnsiColor::BrightCyan),
        "bright_white" => Some(AnsiColor::BrightWhite),
        "default" => None,
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn widget_output_is_not_expanded_again_in_any_pipe_mode_or_cache_path() {
        use crate::{
            pipe::parse_protocol,
            widgets::{pipe::PipeWidget, tabs::TabsWidget},
        };
        use zellij_tile::prelude::TabInfo;

        for (mode, value, expected_tab) in [
            (
                "static",
                "{pipe_global}{tabs}#[bold]value",
                "({pipe_global}{tabs}#[bold]value)",
            ),
            (
                "dynamic",
                "{pipe_global}{tabs}#[bold]value",
                "({pipe_global}{tabs}value)",
            ),
            (
                "raw",
                "\x1b[1m{pipe_global}{tabs}value",
                "{pipe_global}{tabs}value",
            ),
        ] {
            let config = BTreeMap::from([
                ("tab_normal".to_owned(), "{tab_pipe_git}".to_owned()),
                ("tab_pipe_git_format".to_owned(), "({output})".to_owned()),
                ("tab_pipe_git_rendermode".to_owned(), mode.to_owned()),
                ("pipe_global_format".to_owned(), "{output}".to_owned()),
            ]);
            let widgets: BTreeMap<String, Arc<dyn Widget>> = BTreeMap::from([
                (
                    "tabs".to_owned(),
                    Arc::new(TabsWidget::new(&config)) as Arc<dyn Widget>,
                ),
                (
                    "pipe".to_owned(),
                    Arc::new(PipeWidget::new(&config)) as Arc<dyn Widget>,
                ),
            ]);

            for template in [
                "before{tabs}after",
                "{tabs}/{tabs}",
                "{tabs}/{pipe_global}/{tabs}",
            ] {
                let mut state = ZellijState::default();
                state.update_tabs(vec![TabInfo {
                    tab_id: 42,
                    ..Default::default()
                }]);
                parse_protocol(&mut state, &format!("zjstatus::tab_pipe::42::git::{value}"));
                let mut part = FormattedPart::from_format_string(template, &config);

                // Cold render, warmed caches, then a global-only update with cached tabs.
                for global in ["G", "G", "H"] {
                    parse_protocol(
                        &mut state,
                        &format!("zjstatus::pipe::pipe_global::{global}"),
                    );
                    let expected = match template {
                        "before{tabs}after" => format!("before{expected_tab}after"),
                        "{tabs}/{tabs}" => format!("{expected_tab}/{expected_tab}"),
                        _ => format!("{expected_tab}/{global}/{expected_tab}"),
                    };
                    let rendered = part.format_string_with_widgets(&widgets, &state);
                    assert_eq!(
                        console::strip_ansi_codes(&rendered),
                        expected,
                        "mode={mode}, template={template}, global={global}"
                    );
                    assert_eq!(console::strip_ansi_codes(&part.cache["tabs"]), expected_tab);
                    state.cache_mask = 0;
                }
            }
        }
    }

    #[test]
    fn tab_pipe_writes_and_clears_refresh_both_warmed_cache_layers() {
        use crate::{
            pipe::parse_protocol,
            widgets::{pipe::PipeWidget, tabs::TabsWidget},
        };
        use zellij_tile::prelude::TabInfo;

        let config = BTreeMap::from([
            ("tab_normal".to_owned(), "{name}{tab_pipe_git}".to_owned()),
            ("tab_pipe_git_format".to_owned(), "({output})".to_owned()),
            ("pipe_global_format".to_owned(), "{output}".to_owned()),
        ]);
        let widgets: BTreeMap<String, Arc<dyn Widget>> = BTreeMap::from([
            (
                "tabs".to_owned(),
                Arc::new(TabsWidget::new(&config)) as Arc<dyn Widget>,
            ),
            (
                "pipe".to_owned(),
                Arc::new(PipeWidget::new(&config)) as Arc<dyn Widget>,
            ),
        ]);
        // Exercise the outer cache, then the widget cache within an Always fragment.
        for template in ["{tabs}", "{tabs}{pipe_global}"] {
            let mut part = FormattedPart::from_format_string(template, &config);
            let mut state = ZellijState::default();
            state.update_tabs(vec![TabInfo {
                tab_id: 42,
                position: 0,
                name: "tab".to_owned(),
                ..Default::default()
            }]);
            let initial = part.format_string_with_widgets(&widgets, &state);
            assert_eq!(console::strip_ansi_codes(&initial), "tab");
            state.cache_mask = 0;
            assert_eq!(part.format_string_with_widgets(&widgets, &state), initial);
            assert_eq!(part.cached_content, initial);
            assert_eq!(part.cache["tabs"], "tab");

            for (value, expected, changed) in [
                ("main", "tab(main)", true),
                ("main", "tab(main)", false),
                ("next", "tab(next)", true),
                ("", "tab", true),
                ("", "tab", false),
            ] {
                state.cache_mask = 0;
                assert_eq!(
                    parse_protocol(&mut state, &format!("zjstatus::tab_pipe::42::git::{value}")),
                    changed
                );
                let rendered = part.format_string_with_widgets(&widgets, &state);
                assert_eq!(console::strip_ansi_codes(&rendered), expected);
                assert_eq!(console::strip_ansi_codes(&part.cache["tabs"]), expected);
                state.cache_mask = 0;
                assert_eq!(part.format_string_with_widgets(&widgets, &state), rendered);
            }
        }
        let mut outer = FormattedPart::from_format_string("{tab_pipe_git}", &config);
        assert_eq!(outer.cache_mask, UpdateEventMask::None as u8);
        assert_eq!(
            outer.format_string_with_widgets(&widgets, &ZellijState::default()),
            "Use of uninitialized widget"
        );
    }

    #[test]
    fn test_hex_to_rgb() {
        let result = hex_to_rgb("010203");
        let expected = Vec::from([1, 2, 3]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_hex_to_rgb_with_invalid_input() {
        let result = hex_to_rgb("#010203");
        assert!(result.is_err());

        let result = hex_to_rgb(" 010203");
        assert!(result.is_err());

        let result = hex_to_rgb("010");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_color() {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert("color_green".to_owned(), "#00ff00".to_owned());

        let result = parse_color("#010203", &config);
        let expected = RgbColor(1, 2, 3);
        assert_eq!(result, Some(expected.into()));

        let result = parse_color("255", &config);
        let expected = Ansi256Color(255);
        assert_eq!(result, Some(expected.into()));

        let result = parse_color("365", &config);
        assert_eq!(result, None);

        let result = parse_color("#365", &config);
        assert_eq!(result, None);

        let result = parse_color("$green", &config);
        let expected = RgbColor(0, 255, 0);
        assert_eq!(result, Some(expected.into()));

        let result = parse_color("$blue", &config);
        assert_eq!(result, None);
    }
}
