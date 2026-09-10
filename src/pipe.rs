use std::ops::Sub;

use chrono::{Duration, Local};

use crate::{
    config::{UpdateEventMask, ZellijState},
    widgets::{command::TIMESTAMP_FORMAT, notification},
};

/// Parses the line protocol and updates the state accordingly
///
/// The protocol is as follows:
///
/// zjstatus::command_name::args
///
/// It first starts with `zjstatus` as a prefix to indicate that the line is
/// used for the line protocol and zjstatus should parse it. It is followed
/// by the command name and then the arguments. The following commands are
/// available:
///
/// - `rerun` - Reruns the command with the given name (like in the config) as
///             argument. E.g. `zjstatus::rerun::command_1`
/// - `tab_pipe` - Sets or clears a field for a stable tab ID:
///                `zjstatus::tab_pipe::<tab_id>::<field>::<value>`
///
/// The function returns a boolean indicating whether the state has been
/// changed and the UI should be re-rendered.
#[tracing::instrument(skip(state))]
pub fn parse_protocol(state: &mut ZellijState, input: &str) -> bool {
    tracing::debug!("parsing protocol");
    let lines = input.split('\n').collect::<Vec<&str>>();

    let mut should_render = false;
    for line in lines {
        let line_renders = process_line(state, line);

        if line_renders {
            should_render = true;
        }
    }

    should_render
}

#[tracing::instrument(skip_all)]
fn process_line(state: &mut ZellijState, line: &str) -> bool {
    if let Some(args) = line.strip_prefix("zjstatus::tab_pipe::") {
        return tab_pipe(state, args);
    }

    let parts = line.split("::").collect::<Vec<&str>>();

    if parts.len() < 3 {
        return false;
    }

    if parts[0] != "zjstatus" {
        return false;
    }

    tracing::debug!("command: {}", parts[1]);

    let mut should_render = false;
    #[allow(clippy::single_match)]
    match parts[1] {
        "rerun" => {
            rerun_command(state, parts[2]);

            should_render = true;
        }
        "notify" => {
            notify(state, parts[2]);

            should_render = true;
        }
        "pipe" => {
            if parts.len() < 4 {
                return false;
            }

            pipe(state, parts[2], parts[3]);

            should_render = true;
        }
        _ => {}
    }

    should_render
}

fn tab_pipe(state: &mut ZellijState, args: &str) -> bool {
    let mut parts = args.splitn(3, "::");
    let (Some(id), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if id.is_empty()
        || !id.bytes().all(|b| b.is_ascii_digit())
        || field.is_empty()
        || !field
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return false;
    }
    let Ok(id) = id.parse::<usize>() else {
        return false;
    };
    if state.tabs_initialized && !state.tabs.iter().any(|tab| tab.tab_id == id) {
        return false;
    }

    if value.is_empty() {
        let Some(fields) = state.tab_pipe_results.get_mut(&id) else {
            return false;
        };
        if fields.remove(field).is_none() {
            return false;
        }
        if fields.is_empty() {
            state.tab_pipe_results.remove(&id);
        }
    } else {
        let fields = state.tab_pipe_results.entry(id).or_default();
        if fields.get(field).is_some_and(|existing| existing == value) {
            return false;
        }
        fields.insert(field.to_owned(), value.to_owned());
    }

    state.cache_mask |= UpdateEventMask::Tab as u8;
    true
}

fn pipe(state: &mut ZellijState, name: &str, content: &str) {
    tracing::debug!("saving pipe result {name} {content}");
    state
        .pipe_results
        .insert(name.to_owned(), content.to_owned());
}

fn notify(state: &mut ZellijState, message: &str) {
    state.incoming_notification = Some(notification::Message {
        body: message.to_string(),
        received_at: Local::now(),
    });
}

fn rerun_command(state: &mut ZellijState, command_name: &str) {
    invalidate_command_result(state, command_name);
}

/// Backdates the stored timestamp of a command result so the next render
/// re-runs the command.
pub fn invalidate_command_result(state: &mut ZellijState, command_name: &str) {
    let command_result = state.command_results.get(command_name);

    if command_result.is_none() {
        return;
    }

    let mut command_result = command_result.unwrap().clone();

    let ts = Sub::<Duration>::sub(Local::now(), Duration::try_days(1).unwrap());

    command_result.context.insert(
        "timestamp".to_string(),
        ts.format(TIMESTAMP_FORMAT).to_string(),
    );

    state.command_results.remove(command_name);
    state
        .command_results
        .insert(command_name.to_string(), command_result.clone());
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::widgets::command::CommandResult;
    use std::collections::BTreeMap;
    use zellij_tile::prelude::TabInfo;

    #[test]
    fn tab_pipe_values_are_independent_and_preserve_content() {
        let mut state = ZellijState::default();
        assert!(parse_protocol(
            &mut state,
            concat!(
                "zjstatus::tab_pipe::42::git::main\n",
                "zjstatus::tab_pipe::57::git::feat/search\n",
                "zjstatus::tab_pipe::42::build_status:: passing ::one::two \n",
                "zjstatus::tab_pipe::0::0_::  \n",
            )
        ));
        assert_eq!(state.tab_pipe_results[&42]["git"], "main");
        assert_eq!(state.tab_pipe_results[&57]["git"], "feat/search");
        assert_eq!(
            state.tab_pipe_results[&42]["build_status"],
            " passing ::one::two "
        );
        assert_eq!(state.tab_pipe_results[&0]["0_"], "  ");
        assert!(state.pipe_results.is_empty());
        assert!(parse_protocol(
            &mut state,
            &format!("zjstatus::tab_pipe::{}::git::max", usize::MAX)
        ));
        assert_eq!(state.tab_pipe_results[&usize::MAX]["git"], "max");
    }

    #[test]
    fn malformed_tab_pipes_do_not_mutate_or_invalidate() {
        let mut state = ZellijState::default();
        parse_protocol(&mut state, "zjstatus::tab_pipe::42::git::keep");
        state.cache_mask = UpdateEventMask::Session as u8;
        let results = state.tab_pipe_results.clone();
        for line in [
            "",
            "zjstatus::tab_pipe",
            "zjstatus::tab_pipe::",
            "zjstatus::tab_pipe::42",
            "zjstatus::tab_pipe::42::git",
            "zjstatus::tab_pipe::::git::bad",
            "zjstatus::tab_pipe::42::::bad",
            "zjstatus::tab_pipe::42::Git::bad",
            "zjstatus::tab_pipe::42::git-name::bad",
            "zjstatus::tab_pipe::42::git name::bad",
            "zjstatus::tab_pipe::42::g\u{e9}t::bad",
            "zjstatus::tab_pipe::-1::git::bad",
            "zjstatus::tab_pipe::+1::git::bad",
            "zjstatus::tab_pipe:: 42::git::bad",
            "zjstatus::tab_pipe::42 ::git::bad",
            "zjstatus::tab_pipe::1x::git::bad",
            "zjstatus::tab_pipe::1.0::git::bad",
            "zjstatus::tab_pipe::999999999999999999999999999999::git::bad",
            "other::tab_pipe::42::git::bad",
        ] {
            assert!(!parse_protocol(&mut state, line), "{line:?}");
            assert_eq!(state.tab_pipe_results, results, "{line:?}");
            assert_eq!(state.cache_mask, UpdateEventMask::Session as u8);
        }
    }

    #[test]
    fn tab_pipe_changes_invalidate_but_noops_do_not() {
        let mut state = ZellijState::default();
        for (line, changed) in [
            ("zjstatus::tab_pipe::42::git::", false),
            ("zjstatus::tab_pipe::42::git::main", true),
            ("zjstatus::tab_pipe::42::git::main", false),
            ("zjstatus::tab_pipe::42::git::next", true),
            ("zjstatus::tab_pipe::42::build::ok", true),
            ("zjstatus::tab_pipe::42::missing::", false),
            ("zjstatus::tab_pipe::42::git::", true),
            ("zjstatus::tab_pipe::42::git::", false),
        ] {
            state.cache_mask = UpdateEventMask::Session as u8;
            assert_eq!(parse_protocol(&mut state, line), changed, "{line}");
            assert_eq!(
                state.cache_mask,
                UpdateEventMask::Session as u8
                    | if changed {
                        UpdateEventMask::Tab as u8
                    } else {
                        0
                    }
            );
        }
        assert_eq!(
            state.tab_pipe_results[&42],
            BTreeMap::from([("build".to_owned(), "ok".to_owned())])
        );
        assert!(parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::42::build::"
        ));
        assert!(state.tab_pipe_results.is_empty());
    }

    #[test]
    fn tab_snapshots_prune_provisional_and_closed_tabs_and_require_retry() {
        let mut state = ZellijState::default();
        parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::42::git::main\nzjstatus::tab_pipe::57::git::other",
        );
        let tab = TabInfo {
            tab_id: 42,
            position: 0,
            ..Default::default()
        };
        state.update_tabs(vec![tab.clone()]);
        assert!(state.tabs_initialized);
        assert_eq!(state.tab_pipe_results.len(), 1);
        assert_eq!(state.tab_pipe_results[&42]["git"], "main");
        state.cache_mask = 0;
        assert!(!parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::57::git::late"
        ));
        assert_eq!(state.cache_mask, 0);

        let new_tab = TabInfo {
            tab_id: 57,
            position: 1,
            ..Default::default()
        };
        state.update_tabs(vec![tab, new_tab.clone()]);
        assert!(!state.tab_pipe_results.contains_key(&57));
        assert!(parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::57::git::retry"
        ));
        state.update_tabs(vec![new_tab]);
        assert!(!state.tab_pipe_results.contains_key(&42));
        assert_eq!(state.tab_pipe_results[&57]["git"], "retry");
        assert!(!parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::42::git::late"
        ));

        state.update_tabs(vec![]);
        assert!(state.tab_pipe_results.is_empty());
        assert!(!parse_protocol(
            &mut state,
            "zjstatus::tab_pipe::57::git::late"
        ));
        let mut empty_start = ZellijState::default();
        empty_start.update_tabs(vec![]);
        assert!(!parse_protocol(
            &mut empty_start,
            "zjstatus::tab_pipe::0::git::early"
        ));
    }

    #[test]
    fn mixed_batches_and_legacy_commands_keep_their_behavior() {
        let mut state = ZellijState::default();
        state.command_results.insert(
            "command_test".to_owned(),
            CommandResult {
                context: BTreeMap::from([("timestamp".to_owned(), "original".to_owned())]),
                ..Default::default()
            },
        );
        assert!(parse_protocol(
            &mut state,
            concat!(
                "malformed\nzjstatus::tab_pipe::42::git::main\n",
                "zjstatus::tab_pipe::42::git::main\nzjstatus::tab_pipe::42::git\n",
                "zjstatus::pipe::pipe_test::first::discarded\n",
                "zjstatus::notify::hello::discarded\n",
                "zjstatus::rerun::command_test\ninvalid"
            )
        ));
        assert_eq!(state.tab_pipe_results[&42]["git"], "main");
        assert_eq!(state.pipe_results["pipe_test"], "first");
        assert_eq!(state.incoming_notification.as_ref().unwrap().body, "hello");
        assert_ne!(
            state.command_results["command_test"].context["timestamp"],
            "original"
        );
        state.cache_mask = 0;
        assert!(parse_protocol(
            &mut state,
            "zjstatus::pipe::pipe_test::first"
        ));
        assert!(parse_protocol(&mut state, "zjstatus::pipe::pipe_test::"));
        assert_eq!(state.pipe_results["pipe_test"], "");
        assert!(parse_protocol(&mut state, "zjstatus::rerun::unknown"));
        assert!(!parse_protocol(&mut state, "zjstatus::pipe::pipe_test"));
        assert_eq!(state.cache_mask, 0);
    }
}
