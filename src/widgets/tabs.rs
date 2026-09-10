use std::{cmp, collections::BTreeMap};

use lazy_static::lazy_static;
use regex::Regex;
use zellij_tile::prelude::{InputMode, ModeInfo, PaneInfo, PaneManifest, TabInfo};
#[cfg(all(not(feature = "bench"), not(test)))]
use zellij_tile::shim::switch_tab_to;

use crate::{config::ZellijState, render::FormattedPart};

use super::{
    pipe::{PipeConfig, parse_config},
    widget::Widget,
};

lazy_static! {
    static ref TAB_PIPE_REGEX: Regex = Regex::new(r"\{tab_pipe_([a-z_0-9]+)\}").unwrap();
}

pub struct TabsWidget {
    active_tab_format: Vec<FormattedPart>,
    active_tab_fullscreen_format: Vec<FormattedPart>,
    active_tab_sync_format: Vec<FormattedPart>,
    normal_tab_format: Vec<FormattedPart>,
    normal_tab_fullscreen_format: Vec<FormattedPart>,
    normal_tab_sync_format: Vec<FormattedPart>,
    normal_tab_bell_format: Option<Vec<FormattedPart>>,
    normal_tab_flashing_bell_format: Option<Vec<FormattedPart>>,
    rename_tab_format: Vec<FormattedPart>,
    separator: Option<FormattedPart>,
    fullscreen_indicator: Option<String>,
    floating_indicator: Option<String>,
    sync_indicator: Option<String>,
    bell_indicator: Option<String>,
    flashing_bell_indicator: Option<String>,
    tab_display_count: Option<usize>,
    tab_truncate_start_format: Vec<FormattedPart>,
    tab_truncate_end_format: Vec<FormattedPart>,
    tab_zero_based_index: bool,
    tab_pipe_config: BTreeMap<String, PipeConfig>,
    zj_conf: BTreeMap<String, String>,
}

impl TabsWidget {
    pub fn new(config: &BTreeMap<String, String>) -> Self {
        let mut normal_tab_format: Vec<FormattedPart> = Vec::new();
        if let Some(form) = config.get("tab_normal") {
            normal_tab_format = FormattedPart::multiple_from_format_string(form, config);
        }

        let normal_tab_fullscreen_format = match config.get("tab_normal_fullscreen") {
            Some(form) => FormattedPart::multiple_from_format_string(form, config),
            None => normal_tab_format.clone(),
        };

        let normal_tab_sync_format = match config.get("tab_normal_sync") {
            Some(form) => FormattedPart::multiple_from_format_string(form, config),
            None => normal_tab_format.clone(),
        };

        let normal_tab_bell_format = config
            .get("tab_normal_bell")
            .map(|form| FormattedPart::multiple_from_format_string(form, config));

        let normal_tab_flashing_bell_format = config
            .get("tab_normal_flashing_bell")
            .map(|form| FormattedPart::multiple_from_format_string(form, config));

        let mut active_tab_format = normal_tab_format.clone();
        if let Some(form) = config.get("tab_active") {
            active_tab_format = FormattedPart::multiple_from_format_string(form, config);
        }

        let active_tab_fullscreen_format = match config.get("tab_active_fullscreen") {
            Some(form) => FormattedPart::multiple_from_format_string(form, config),
            None => active_tab_format.clone(),
        };

        let active_tab_sync_format = match config.get("tab_active_sync") {
            Some(form) => FormattedPart::multiple_from_format_string(form, config),
            None => active_tab_format.clone(),
        };

        let rename_tab_format = match config.get("tab_rename") {
            Some(form) => FormattedPart::multiple_from_format_string(form, config),
            None => active_tab_format.clone(),
        };

        let tab_display_count = match config.get("tab_display_count") {
            Some(count) => count.parse::<usize>().ok(),
            None => None,
        };

        let tab_truncate_start_format = config
            .get("tab_truncate_start_format")
            .map(|form| FormattedPart::multiple_from_format_string(form, config))
            .unwrap_or_default();

        let tab_truncate_end_format = config
            .get("tab_truncate_end_format")
            .map(|form| FormattedPart::multiple_from_format_string(form, config))
            .unwrap_or_default();

        let tab_zero_based_index = match config.get("tab_zero_based_index") {
            Some(e) => matches!(e.as_str(), "true"),
            None => false,
        };

        let separator = config
            .get("tab_separator")
            .map(|s| FormattedPart::from_format_string(s, config));

        let bell_indicator = config.get("tab_bell_indicator").cloned();
        let flashing_bell_indicator = config
            .get("tab_flashing_bell_indicator")
            .cloned()
            .or_else(|| bell_indicator.clone());

        Self {
            normal_tab_format,
            normal_tab_fullscreen_format,
            normal_tab_sync_format,
            normal_tab_bell_format,
            normal_tab_flashing_bell_format,
            active_tab_format,
            active_tab_fullscreen_format,
            active_tab_sync_format,
            rename_tab_format,
            separator,
            floating_indicator: config.get("tab_floating_indicator").cloned(),
            sync_indicator: config.get("tab_sync_indicator").cloned(),
            fullscreen_indicator: config.get("tab_fullscreen_indicator").cloned(),
            bell_indicator,
            flashing_bell_indicator,
            tab_display_count,
            tab_truncate_start_format,
            tab_truncate_end_format,
            tab_zero_based_index,
            tab_pipe_config: parse_config(config, "tab_pipe_"),
            zj_conf: config.clone(),
        }
    }
}

impl Widget for TabsWidget {
    fn process(&self, _name: &str, state: &ZellijState) -> String {
        let mut output = "".to_owned();
        let mut counter = 0;

        let (truncated_start, truncated_end, tabs) =
            get_tab_window(&state.tabs, self.tab_display_count);

        if truncated_start > 0 {
            for f in &self.tab_truncate_start_format {
                let mut content = f.content.clone();

                if content.contains("{count}") {
                    content = content.replace("{count}", (truncated_start).to_string().as_str());
                }

                output = format!("{output}{}", f.format_string(&content));
            }
        }

        for tab in &tabs {
            let content = self.render_tab(tab, state);
            counter += 1;

            output = format!("{}{}", output, content);

            if counter < tabs.len()
                && let Some(sep) = &self.separator
            {
                output = format!("{}{}", output, sep.format_string(&sep.content));
            }
        }

        if truncated_end > 0 {
            for f in &self.tab_truncate_end_format {
                let mut content = f.content.clone();

                if content.contains("{count}") {
                    content = content.replace("{count}", (truncated_end).to_string().as_str());
                }

                output = format!("{output}{}", f.format_string(&content));
            }
        }

        output
    }

    fn process_click(&self, _name: &str, state: &ZellijState, pos: usize) {
        #[cfg(any(feature = "bench", test))]
        let switch_tab_to = |_| {};
        self.process_click_with(state, pos, switch_tab_to);
    }
}

impl TabsWidget {
    fn process_click_with(&self, state: &ZellijState, pos: usize, mut switch: impl FnMut(u32)) {
        let mut offset = 0;
        let mut counter = 0;

        let (truncated_start, truncated_end, tabs) =
            get_tab_window(&state.tabs, self.tab_display_count);

        let active_pos = &state
            .tabs
            .iter()
            .find(|t| t.active)
            .expect("no active tab")
            .position
            + 1;

        if truncated_start > 0 {
            for f in &self.tab_truncate_start_format {
                let mut content = f.content.clone();

                if content.contains("{count}") {
                    content = content.replace("{count}", (truncated_end).to_string().as_str());
                }

                offset += console::measure_text_width(&f.format_string(&content));

                if pos <= offset {
                    switch(active_pos.saturating_sub(1) as u32);
                }
            }
        }

        for tab in &tabs {
            counter += 1;

            let mut rendered_content = self.render_tab(tab, state);

            if counter < tabs.len()
                && let Some(sep) = &self.separator
            {
                rendered_content =
                    format!("{}{}", rendered_content, sep.format_string(&sep.content));
            }

            let content_len = console::measure_text_width(&rendered_content);

            if pos > offset && pos < offset + content_len {
                switch(tab.position as u32 + 1);

                break;
            }

            offset += content_len;
        }

        if truncated_end > 0 {
            for f in &self.tab_truncate_end_format {
                let mut content = f.content.clone();

                if content.contains("{count}") {
                    content = content.replace("{count}", (truncated_end).to_string().as_str());
                }

                offset += console::measure_text_width(&f.format_string(&content));

                if pos <= offset {
                    switch(cmp::min(active_pos + 1, state.tabs.len()) as u32);
                }
            }
        }
    }
}

impl TabsWidget {
    fn select_format(&self, info: &TabInfo, mode: &ModeInfo) -> &Vec<FormattedPart> {
        if info.active && mode.mode == InputMode::RenameTab {
            return &self.rename_tab_format;
        }

        if !info.active && info.is_flashing_bell {
            let fmt = self
                .normal_tab_flashing_bell_format
                .as_ref()
                .or(self.normal_tab_bell_format.as_ref());
            if let Some(fmt) = fmt {
                return fmt;
            }
        }

        if !info.active
            && info.has_bell_notification
            && let Some(fmt) = self.normal_tab_bell_format.as_ref()
        {
            return fmt;
        }

        if info.active && info.is_fullscreen_active {
            return &self.active_tab_fullscreen_format;
        }

        if info.active && info.is_sync_panes_active {
            return &self.active_tab_sync_format;
        }

        if info.active {
            return &self.active_tab_format;
        }

        if info.is_fullscreen_active {
            return &self.normal_tab_fullscreen_format;
        }

        if info.is_sync_panes_active {
            return &self.normal_tab_sync_format;
        }

        &self.normal_tab_format
    }

    fn render_tab(&self, tab: &TabInfo, state: &ZellijState) -> String {
        let panes = &state.panes;
        let mode = &state.mode;
        let formatters = self.select_format(tab, mode);
        let mut output = "".to_owned();

        let render_template_span = |f: &FormattedPart, template: &str| {
            let mut content = template.to_owned();

            let tab_name = match mode.mode {
                InputMode::RenameTab => match tab.name.is_empty() {
                    true => "Enter name...",
                    false => tab.name.as_str(),
                },
                _name => tab.name.as_str(),
            };

            if content.contains("{name}") {
                content = content.replace("{name}", tab_name);
            }

            if content.contains("{index}") {
                let index = match self.tab_zero_based_index {
                    true => tab.position,
                    false => tab.position + 1,
                };
                content = content.replace("{index}", index.to_string().as_str());
            }

            if content.contains("{floating_total_count}") {
                let panes_for_tab: Vec<PaneInfo> =
                    panes.panes.get(&tab.position).cloned().unwrap_or_default();

                content = content.replace(
                    "{floating_total_count}",
                    &format!("{}", panes_for_tab.iter().filter(|p| p.is_floating).count()),
                );
            }

            if content.contains("{focused_pane_title}") {
                let panes_for_tab: Vec<PaneInfo> =
                    panes.panes.get(&tab.position).cloned().unwrap_or_default();

                let focused_pane_title = panes_for_tab
                    .iter()
                    .find(|pane| pane.is_focused)
                    .map(|pane| pane.title.clone())
                    .unwrap_or_default();

                content = content.replace("{focused_pane_title}", &focused_pane_title);
            }

            content = self.replace_indicators(content, tab, panes);

            f.format_string(&content)
        };

        for f in formatters {
            let mut end = 0;
            // Scan only the original template, never producer output or substituted tab names.
            for capture in TAB_PIPE_REGEX.captures_iter(&f.content) {
                let placeholder = capture.get(0).unwrap();
                output.push_str(&render_template_span(
                    f,
                    &f.content[end..placeholder.start()],
                ));

                let key = &placeholder.as_str()[1..placeholder.len() - 1];
                if let Some(config) = self.tab_pipe_config.get(key)
                    && let Some(value) = state
                        .tab_pipe_results
                        .get(&tab.tab_id)
                        .and_then(|fields| fields.get(&capture[1]))
                        .filter(|value| !value.is_empty())
                {
                    output.push_str(&config.render_in_tab(value, &self.zj_conf, f));
                }
                end = placeholder.end();
            }
            output.push_str(&render_template_span(f, &f.content[end..]));
        }

        output.to_owned()
    }

    fn replace_indicators(&self, content: String, tab: &TabInfo, panes: &PaneManifest) -> String {
        let mut content = content;
        if content.contains("{fullscreen_indicator}")
            && let Some(fullscreen_indicator) = self.fullscreen_indicator.clone()
        {
            content = content.replace(
                "{fullscreen_indicator}",
                if tab.is_fullscreen_active {
                    fullscreen_indicator.as_ref()
                } else {
                    ""
                },
            );
        }

        if content.contains("{sync_indicator}")
            && let Some(sync_indicator) = self.sync_indicator.clone()
        {
            content = content.replace(
                "{sync_indicator}",
                if tab.is_sync_panes_active {
                    sync_indicator.as_ref()
                } else {
                    ""
                },
            );
        }

        if content.contains("{floating_indicator}")
            && let Some(floating_indicator) = self.floating_indicator.clone()
        {
            let panes_for_tab: Vec<PaneInfo> =
                panes.panes.get(&tab.position).cloned().unwrap_or_default();

            let is_floating = panes_for_tab.iter().any(|p| p.is_floating);

            content = content.replace(
                "{floating_indicator}",
                if is_floating {
                    floating_indicator.as_ref()
                } else {
                    ""
                },
            );
        }

        if content.contains("{bell_indicator}")
            && (self.bell_indicator.is_some() || self.flashing_bell_indicator.is_some())
        {
            let indicator = if tab.is_flashing_bell {
                self.flashing_bell_indicator.as_deref().unwrap_or("")
            } else if tab.has_bell_notification {
                self.bell_indicator.as_deref().unwrap_or("")
            } else {
                ""
            };

            content = content.replace("{bell_indicator}", indicator);
        }

        content
    }
}

pub fn get_tab_window(
    tabs: &Vec<TabInfo>,
    max_count: Option<usize>,
) -> (usize, usize, Vec<TabInfo>) {
    let max_count = match max_count {
        Some(count) => count,
        None => return (0, 0, tabs.to_vec()),
    };

    if tabs.len() <= max_count {
        return (0, 0, tabs.to_vec());
    }

    let active_index = tabs.iter().position(|t| t.active).expect("no active tab");

    // active tab is in the last #max_count tabs, so return the last #max_count
    if active_index > tabs.len().saturating_sub(max_count) {
        return (
            tabs.len().saturating_sub(max_count),
            0,
            tabs.iter()
                .cloned()
                .rev()
                .take(max_count)
                .rev()
                .collect::<Vec<TabInfo>>(),
        );
    }

    // tabs must be truncated
    let first_index = active_index.saturating_sub(1);
    let last_index = cmp::min(first_index + max_count, tabs.len());

    (
        first_index,
        tabs.len().saturating_sub(last_index),
        tabs.as_slice()[first_index..last_index].to_vec(),
    )
}

#[cfg(test)]
mod test {
    use zellij_tile::prelude::TabInfo;

    use super::*;
    use crate::pipe::parse_protocol;
    use rstest::rstest;

    fn pipe_fixture() -> (BTreeMap<String, String>, ZellijState) {
        let config = BTreeMap::from([
            (
                "tab_normal".to_owned(),
                "{index}:{name}{tab_pipe_git}".to_owned(),
            ),
            ("tab_separator".to_owned(), " | ".to_owned()),
            ("tab_pipe_git_format".to_owned(), "({output})".to_owned()),
        ]);
        let mut state = ZellijState::default();
        state.update_tabs(vec![
            TabInfo {
                tab_id: 42,
                position: 0,
                name: "same".to_owned(),
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 57,
                position: 1,
                name: "same".to_owned(),
                ..Default::default()
            },
        ]);
        parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::42::git::main\nzjstatus::tab_pipe::57::git::other",
        );
        (config, state)
    }

    #[rstest]
    #[case("tab_normal", false, false, false, false, false, false)]
    #[case("tab_active", true, false, false, false, false, false)]
    #[case("tab_normal_fullscreen", false, true, false, false, false, false)]
    #[case("tab_active_fullscreen", true, true, false, false, false, false)]
    #[case("tab_normal_sync", false, false, true, false, false, false)]
    #[case("tab_active_sync", true, false, true, false, false, false)]
    #[case("tab_normal_bell", false, false, false, true, false, false)]
    #[case("tab_normal_flashing_bell", false, false, false, true, true, false)]
    #[case("tab_rename", true, false, false, false, false, true)]
    fn tab_pipes_resolve_in_every_format(
        #[case] key: &str,
        #[case] active: bool,
        #[case] fullscreen: bool,
        #[case] sync: bool,
        #[case] bell: bool,
        #[case] flashing: bool,
        #[case] rename: bool,
    ) {
        let (mut config, mut state) = pipe_fixture();
        config.insert(key.to_owned(), format!("{key}:{{tab_pipe_git}}"));
        if rename {
            state.mode.mode = InputMode::RenameTab;
        }
        let tab = TabInfo {
            tab_id: 57,
            position: 8,
            active,
            is_fullscreen_active: fullscreen,
            is_sync_panes_active: sync,
            has_bell_notification: bell,
            is_flashing_bell: flashing,
            ..Default::default()
        };
        let widget = TabsWidget::new(&config);
        assert_eq!(
            console::strip_ansi_codes(&widget.render_tab(&tab, &state)),
            format!("{key}:(other)")
        );
    }

    #[test]
    fn identity_survives_reorder_rename_active_changes_and_hidden_tabs() {
        let (mut config, mut state) = pipe_fixture();
        let widget = TabsWidget::new(&config);
        assert_eq!(
            console::strip_ansi_codes(&widget.process("tabs", &state)),
            "1:same(main) | 2:same(other)"
        );
        state.tabs.swap(0, 1);
        for (position, tab) in state.tabs.iter_mut().enumerate() {
            tab.position = position;
            tab.active = !tab.active;
        }
        state.tabs[0].name = "renamed".to_owned();
        state.update_tabs(state.tabs.clone());
        assert_eq!(
            console::strip_ansi_codes(&widget.process("tabs", &state)),
            "1:renamed(other) | 2:same(main)"
        );
        config.insert("tab_zero_based_index".to_owned(), "true".to_owned());
        let widget = TabsWidget::new(&config);
        assert_eq!(
            console::strip_ansi_codes(&widget.process("tabs", &state)),
            "0:renamed(other) | 1:same(main)"
        );
        config.insert("tab_display_count".to_owned(), "1".to_owned());
        let widget = TabsWidget::new(&config);
        assert_eq!(
            console::strip_ansi_codes(&widget.process("tabs", &state)),
            "0:renamed(other)"
        );
        parse_protocol(&mut state, "zjstatus::tab_pipe::42::git::hidden");
        state.tabs[0].active = false;
        state.tabs[1].active = true;
        assert!(widget.render_tab(&state.tabs[1], &state).contains("hidden"));
        assert_eq!(state.tab_pipe_results[&57]["git"], "other");
    }

    #[test]
    fn placeholders_use_original_spans_and_missing_values_have_no_wrappers() {
        let (mut config, mut state) = pipe_fixture();
        config.insert("tab_normal".to_owned(), "{tab_pipe_git}{tab_pipe_build_status}/{name}/{tab_pipe_git}/{tab_pipe_missing}/{tab_pipe_absent}".to_owned());
        config.insert(
            "tab_pipe_build_status_format".to_owned(),
            "<{output}>".to_owned(),
        );
        config.insert("tab_pipe_absent_format".to_owned(), "({output})".to_owned());
        let value = "{tab_pipe_build_status}{name}{output} {index}";
        parse_protocol(
            &mut state,
            &format!(
                "zjstatus::tab_pipe::42::git::{value}\nzjstatus::tab_pipe::42::build_status::ok\nzjstatus::tab_pipe::42::missing::unconfigured"
            ),
        );
        state.tabs[0].name = "{tab_pipe_git}".to_owned();
        let widget = TabsWidget::new(&config);
        assert_eq!(
            console::strip_ansi_codes(&widget.render_tab(&state.tabs[0], &state)),
            format!("({value})<ok>/{{tab_pipe_git}}/({value})//")
        );
        parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::42::git::\nzjstatus::tab_pipe::42::build_status::",
        );
        assert_eq!(
            console::strip_ansi_codes(&widget.render_tab(&state.tabs[0], &state)),
            "/{tab_pipe_git}///"
        );
        for mode in ["static", "dynamic", "raw"] {
            config.insert("tab_pipe_git_rendermode".to_owned(), mode.to_owned());
            assert_eq!(
                console::strip_ansi_codes(
                    &TabsWidget::new(&config).render_tab(&state.tabs[0], &state)
                ),
                "/{tab_pipe_git}///"
            );
        }
    }

    #[test]
    fn tab_pipe_modes_inherit_styles_and_restore_tab_text() {
        for (mode, value, visible) in [
            ("static", "#[bold]value", "(#[bold]value)"),
            ("dynamic", "#[bold]value {name}", "(value {name})"),
            ("raw", "\x1b[32mraw", "raw"),
        ] {
            let (mut config, mut state) = pipe_fixture();
            config.insert(
                "tab_normal".to_owned(),
                "#[fg=red,bg=black,bold]before{tab_pipe_git}after#[fg=blue]next".to_owned(),
            );
            config.insert("tab_pipe_git_rendermode".to_owned(), mode.to_owned());
            parse_protocol(&mut state, &format!("zjstatus::tab_pipe::42::git::{value}"));
            let widget = TabsWidget::new(&config);
            let output = widget.render_tab(&state.tabs[0], &state);
            assert_eq!(
                console::strip_ansi_codes(&output),
                format!("before{visible}afternext")
            );
            let outer = FormattedPart::from_format_string("#[fg=red,bg=black,bold]", &config);
            let next = FormattedPart::from_format_string("#[fg=blue]", &config);
            let expected_pipe = match mode {
                "static" => outer.format_string("(#[bold]value)"),
                "dynamic" => format!(
                    "{}{}",
                    outer.format_string("("),
                    outer.format_string("value {name})")
                ),
                _ => "\x1b[0m\x1b[32mraw\x1b[0m".to_owned(),
            };
            assert_eq!(
                output,
                format!(
                    "{}{}{}{}",
                    outer.format_string("before"),
                    expected_pipe,
                    outer.format_string("after"),
                    next.format_string("next")
                )
            );

            config.insert(
                "tab_normal".to_owned(),
                "{tab_pipe_git}{tab_pipe_git}plain".to_owned(),
            );
            let widget = TabsWidget::new(&config);
            let output = widget.render_tab(&state.tabs[0], &state);
            assert_eq!(
                console::strip_ansi_codes(&output),
                format!("{visible}{visible}plain")
            );
            config.insert("tab_normal".to_owned(), "{tab_pipe_git}".to_owned());
            let widget = TabsWidget::new(&config);
            assert_eq!(
                console::strip_ansi_codes(&widget.render_tab(&state.tabs[0], &state)),
                visible
            );
        }
    }

    #[test]
    fn adjacent_pipes_inherit_active_normal_and_bell_styles_independently() {
        for (key, active, bell, style) in [
            ("tab_active", true, false, "#[bg=#555555,fg=#ffffff,bold]"),
            ("tab_normal", false, false, "#[bg=#222222,fg=#aaaaaa]"),
            ("tab_normal_bell", false, true, "#[bg=red,fg=white,italic]"),
        ] {
            let (mut config, mut state) = pipe_fixture();
            config.insert(
                key.to_owned(),
                format!(
                    "{style} BEFORE{{tab_pipe_git}}{{tab_pipe_plain}}{{tab_pipe_absent}} AFTER "
                ),
            );
            config.insert(
                "tab_pipe_git_format".to_owned(),
                "#[fg=blue] [{output}]".to_owned(),
            );
            config.insert("tab_pipe_plain_format".to_owned(), " [{output}]".to_owned());
            config.insert(
                "tab_pipe_absent_format".to_owned(),
                " [{output}]".to_owned(),
            );
            state.tabs[0].active = active;
            state.tabs[0].has_bell_notification = bell;
            parse_protocol(&mut state, "zjstatus::tab_pipe::42::plain::hello");
            let widget = TabsWidget::new(&config);
            let outer = FormattedPart::from_format_string(style, &config);
            let mut badge = outer.clone();
            badge.fg = FormattedPart::from_format_string("#[fg=blue]", &config).fg;
            let expected = format!(
                "{}{}{}{}{}{}",
                outer.format_string(" BEFORE"),
                badge.format_string(" [main]"),
                outer.format_string(""),
                outer.format_string(" [hello]"),
                outer.format_string(""),
                outer.format_string(" AFTER ")
            );
            assert_eq!(widget.render_tab(&state.tabs[0], &state), expected, "{key}");
            parse_protocol(
                &mut state,
                "zjstatus::tab_pipe::42::git::\nzjstatus::tab_pipe::42::plain::",
            );
            let output = widget.render_tab(&state.tabs[0], &state);
            assert_eq!(console::strip_ansi_codes(&output), " BEFORE AFTER ");
            assert!(!output.contains("\x1b[34m"));
        }
    }

    #[test]
    fn widths_and_click_targets_follow_the_same_rendered_values() {
        let (mut config, mut state) = pipe_fixture();
        config.insert("tab_normal".to_owned(), " {tab_pipe_git} ".to_owned());
        for (mode, value, width) in [
            ("static", "text", 8),
            ("dynamic", "#[bold]text", 8),
            ("raw", "\x1b[31m\u{754c}", 4),
        ] {
            config.insert("tab_pipe_git_rendermode".to_owned(), mode.to_owned());
            parse_protocol(&mut state, &format!("zjstatus::tab_pipe::42::git::{value}"));
            let widget = TabsWidget::new(&config);
            assert_eq!(
                console::measure_text_width(&widget.render_tab(&state.tabs[0], &state)),
                width
            );
            let second_start = width + 3;
            let mut targets = vec![];
            widget.process_click_with(&state, width - 1, |id| targets.push(id));
            widget.process_click_with(&state, second_start + 1, |id| targets.push(id));
            assert_eq!(targets, [1, 2]);
            assert_eq!(
                console::measure_text_width(&widget.process("tabs", &state)),
                second_start
                    + console::measure_text_width(&widget.render_tab(&state.tabs[1], &state))
            );
        }
    }

    #[test]
    fn fallback_precedence_and_no_placeholder_ansi_are_unchanged() {
        let (mut config, mut state) = pipe_fixture();
        config.insert(
            "tab_normal".to_owned(),
            "#[fg=red]{index}:{name}".to_owned(),
        );
        let widget = TabsWidget::new(&config);
        let expected =
            FormattedPart::from_format_string("#[fg=red]", &config).format_string("1:same");
        assert_eq!(widget.render_tab(&state.tabs[0], &state), expected);
        state.mode.mode = InputMode::RenameTab;
        assert_eq!(widget.render_tab(&state.tabs[0], &state), expected);

        config.insert(
            "tab_normal_bell".to_owned(),
            "bell{tab_pipe_git}".to_owned(),
        );
        config.insert(
            "tab_normal_fullscreen".to_owned(),
            "fullscreen{tab_pipe_git}".to_owned(),
        );
        let widget = TabsWidget::new(&config);
        let tab = &mut state.tabs[1];
        tab.is_flashing_bell = true;
        tab.is_fullscreen_active = true;
        tab.is_sync_panes_active = true;
        assert_eq!(
            console::strip_ansi_codes(&widget.render_tab(&state.tabs[1], &state)),
            "bell(other)"
        );
        state.tabs[1].is_flashing_bell = false;
        assert_eq!(
            console::strip_ansi_codes(&widget.render_tab(&state.tabs[1], &state)),
            "fullscreen(other)"
        );
    }

    #[test]
    fn tab_pipes_do_not_expand_in_separators_or_truncation_summaries() {
        let (mut config, state) = pipe_fixture();
        config.insert("tab_separator".to_owned(), "{tab_pipe_git}".to_owned());
        let widget = TabsWidget::new(&config);
        assert!(widget.process("tabs", &state).contains("{tab_pipe_git}"));
        config.insert("tab_display_count".to_owned(), "1".to_owned());
        config.insert(
            "tab_truncate_end_format".to_owned(),
            "{count}{tab_pipe_git}".to_owned(),
        );
        let widget = TabsWidget::new(&config);
        assert!(widget.process("tabs", &state).ends_with("1{tab_pipe_git}"));
    }

    #[test]
    fn tab_pipe_wrappers_and_values_preserve_literal_brackets() {
        let (mut config, mut state) = pipe_fixture();
        config.insert("tab_pipe_git_format".to_owned(), " [{output}]".to_owned());
        parse_protocol(&mut state, "zjstatus::tab_pipe::42::git::[main] {output}");
        for mode in ["static", "dynamic"] {
            config.insert("tab_pipe_git_rendermode".to_owned(), mode.to_owned());
            let widget = TabsWidget::new(&config);
            assert_eq!(
                console::strip_ansi_codes(&widget.render_tab(&state.tabs[0], &state)),
                "1:same [[main] {output}]"
            );
        }
    }

    #[rstest]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (1, 1, vec![
                TabInfo {
                    active: false,
                    name: "2".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: true,
                    name: "3".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: false,
                    name: "4".to_owned(),
                    ..TabInfo::default()
                },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: true,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (0, 2, vec![
                TabInfo {
                    active: true,
                    name: "1".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: false,
                    name: "2".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: false,
                    name: "3".to_owned(),
                    ..TabInfo::default()
                },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (0, 2, vec![
                TabInfo {
                    active: false,
                    name: "1".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: true,
                    name: "2".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: false,
                    name: "3".to_owned(),
                    ..TabInfo::default()
                },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (2, 0, vec![
                TabInfo {
                    active: false,
                    name: "3".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: false,
                    name: "4".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: true,
                    name: "5".to_owned(),
                    ..TabInfo::default()
                },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (2, 0, vec![
                TabInfo {
                    active: false,
                    name: "3".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: true,
                    name: "4".to_owned(),
                    ..TabInfo::default()
                },
                TabInfo {
                    active: false,
                    name: "5".to_owned(),
                    ..TabInfo::default()
                },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
        ],
        None,
        (0, 0, vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "4".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "5".to_owned(),
                ..TabInfo::default()
            },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (0, 0, vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            ]
        )
    )]
    #[case(
        vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
        ],
        Some(3),
        (0, 0, vec![
            TabInfo {
                active: false,
                name: "1".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: true,
                name: "2".to_owned(),
                ..TabInfo::default()
            },
            TabInfo {
                active: false,
                name: "3".to_owned(),
                ..TabInfo::default()
            },
            ]
        )
    )]
    pub fn test_get_tab_window(
        #[case] tabs: Vec<TabInfo>,
        #[case] max_count: Option<usize>,
        #[case] expected: (usize, usize, Vec<TabInfo>),
    ) {
        let res = get_tab_window(&tabs, max_count);

        assert_eq!(res, expected);
    }
}
