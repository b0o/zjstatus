use std::collections::BTreeMap;

use crate::render::{FormattedPart, formatted_parts_from_string_cached};

use super::widget::Widget;

#[derive(Clone, Debug, PartialEq)]
enum RenderMode {
    Static,
    Dynamic,
    Raw,
}

pub struct PipeWidget {
    config: BTreeMap<String, PipeConfig>,
    zj_conf: BTreeMap<String, String>,
}

#[derive(Clone)]
pub(crate) struct PipeConfig {
    format: Vec<FormattedPart>,
    render_mode: RenderMode,
    literal_prefix: bool,
}

impl PipeWidget {
    pub fn new(config: &BTreeMap<String, String>) -> Self {
        Self {
            config: parse_config(config, "pipe_"),
            zj_conf: config.clone(),
        }
    }
}

impl Widget for PipeWidget {
    fn process(&self, name: &str, state: &crate::config::ZellijState) -> String {
        let pipe_config = match self.config.get(name) {
            Some(pc) => pc,
            None => {
                tracing::debug!("pipe no name {name}");
                return "".to_owned();
            }
        };

        let pipe_result = match state.pipe_results.get(name) {
            Some(pr) => pr,
            None => {
                tracing::debug!("pipe no content {name}");
                return "".to_owned();
            }
        };

        pipe_config.render(pipe_result, &self.zj_conf)
    }

    fn process_click(&self, _name: &str, _state: &crate::config::ZellijState, _pos: usize) {}
}

impl PipeConfig {
    pub(crate) fn render(&self, pipe_result: &str, zj_conf: &BTreeMap<String, String>) -> String {
        let content = self
            .format
            .iter()
            .map(|f| {
                let mut content = f.content.clone();

                if content.contains("{output}") {
                    content = content.replace(
                        "{output}",
                        pipe_result.strip_suffix('\n').unwrap_or(pipe_result),
                    )
                }

                (f, content)
            })
            .fold("".to_owned(), |acc, (f, content)| {
                if self.render_mode == RenderMode::Static {
                    return format!("{acc}{}", f.format_string(&content));
                }

                format!("{acc}{}", content)
            });

        match self.render_mode {
            RenderMode::Static => content,
            RenderMode::Dynamic => {
                let content = if self.literal_prefix {
                    format!("#[]{content}")
                } else {
                    content
                };
                render_dynamic_formatted_content(&content, zj_conf)
            }
            RenderMode::Raw => pipe_result.to_owned(),
        }
    }
}

fn render_dynamic_formatted_content(content: &str, config: &BTreeMap<String, String>) -> String {
    formatted_parts_from_string_cached(content, config)
        .iter()
        .map(|fp| fp.format_string(&fp.content))
        .collect::<Vec<String>>()
        .join("")
}

pub(crate) fn parse_config(
    zj_conf: &BTreeMap<String, String>,
    prefix: &str,
) -> BTreeMap<String, PipeConfig> {
    let mut config: BTreeMap<String, PipeConfig> = BTreeMap::new();

    for (key, value) in zj_conf.iter().filter(|(key, _)| key.starts_with(prefix)) {
        let Some(pipe_name) = key
            .strip_suffix("_format")
            .or_else(|| key.strip_suffix("_rendermode"))
        else {
            continue;
        };
        let pipe_conf = config
            .entry(pipe_name.to_owned())
            .or_insert_with(|| PipeConfig {
                format: vec![],
                render_mode: RenderMode::Static,
                literal_prefix: prefix == "tab_pipe_",
            });

        if key.ends_with("_format") {
            // A neutral style marker protects literal ']' in the first text span.
            // Global pipes retain their legacy interpretation of bare style prefixes.
            let value = if pipe_conf.literal_prefix {
                format!("#[]{value}")
            } else {
                value.clone()
            };
            pipe_conf.format = FormattedPart::multiple_from_format_string(&value, zj_conf);
        }

        if key.ends_with("_rendermode") {
            pipe_conf.render_mode = match value.as_str() {
                "dynamic" => RenderMode::Dynamic,
                "raw" => RenderMode::Raw,
                _ => RenderMode::Static,
            };
        }
    }
    config
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::ZellijState;

    #[test]
    fn shared_configuration_and_rendering_preserve_global_pipe_modes() {
        for (mode, expected) in [
            ("static", "(#[bold]value {name} {output})"),
            ("invalid", "(#[bold]value {name} {output})"),
            ("dynamic", "(value {name} {output})"),
            ("raw", "#[bold]value {name} {output}\n"),
        ] {
            let config = BTreeMap::from([
                (
                    "pipe_build_status_format".to_owned(),
                    "({output})".to_owned(),
                ),
                ("pipe_build_status_rendermode".to_owned(), mode.to_owned()),
                (
                    "tab_pipe_build_status_format".to_owned(),
                    "ignored".to_owned(),
                ),
                (
                    "tab_pipe_build_status_unknown_suffix".to_owned(),
                    "ignored".to_owned(),
                ),
            ]);
            let mut state = ZellijState::default();
            state.pipe_results.insert(
                "pipe_build_status".to_owned(),
                "#[bold]value {name} {output}\n".to_owned(),
            );
            let widget = PipeWidget::new(&config);
            assert_eq!(
                console::strip_ansi_codes(&widget.process("pipe_build_status", &state)),
                expected,
                "{mode}"
            );
            assert_eq!(widget.process("tab_pipe_build_status", &state), "");
            assert_eq!(parse_config(&config, "tab_pipe_").len(), 1);
            state.pipe_results.clear();
            assert_eq!(widget.process("pipe_build_status", &state), "");
        }
    }

    #[test]
    fn missing_formats_and_default_mode_keep_existing_semantics() {
        for (mode, expected) in [
            ("static", ""),
            ("dynamic", ""),
            ("invalid", ""),
            ("raw", "value"),
        ] {
            let config = BTreeMap::from([("pipe_test_rendermode".to_owned(), mode.to_owned())]);
            assert_eq!(
                parse_config(&config, "pipe_")["pipe_test"].render("value", &config),
                expected
            );
        }
        let config = BTreeMap::from([("pipe_test_format".to_owned(), "({output})".to_owned())]);
        let pipe = &parse_config(&config, "pipe_")["pipe_test"];
        assert_eq!(pipe.render("#[bold]value", &config), "(#[bold]value)");
        assert_eq!(pipe.render("", &config), "()");
    }
}
