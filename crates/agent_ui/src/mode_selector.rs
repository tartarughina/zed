use std::{rc::Rc, sync::Arc};

use acp_thread::AgentSessionModes;
use agent_client_protocol as acp;
use agent_servers::AgentServer;
use agent_settings::AgentSettings;
use collections::HashSet;
use fs::Fs;
use fuzzy::{StringMatchCandidate, match_strings};
use gpui::{DismissEvent, Subscription, Task};
use picker::{Picker, PickerDelegate};
use settings::{Settings, SettingsStore};
use ui::{DocumentationAside, DocumentationSide, prelude::*};

use crate::{CycleFavoriteModes, ToggleProfileSelector};

use crate::ui::HoldForDefault;

pub type ModeSelector = Picker<ModePickerDelegate>;

pub fn acp_mode_selector(
    session_modes: Rc<dyn AgentSessionModes>,
    agent_server: Rc<dyn AgentServer>,
    fs: Arc<dyn Fs>,
    window: &mut Window,
    cx: &mut Context<ModeSelector>,
) -> ModeSelector {
    let delegate = ModePickerDelegate::new(session_modes, agent_server, fs, window, cx);
    Picker::list(delegate, window, cx)
        .show_scrollbar(true)
        .width(rems(20.))
        .max_height(Some(rems(20.).into()))
}

enum ModePickerEntry {
    Separator(SharedString),
    Mode(acp::SessionMode, bool),
}

pub struct ModePickerDelegate {
    session_modes: Rc<dyn AgentSessionModes>,
    agent_server: Rc<dyn AgentServer>,
    fs: Arc<dyn Fs>,
    filtered_entries: Vec<ModePickerEntry>,
    selected_index: usize,
    selected_description: Option<(usize, SharedString, bool)>,
    pub favorites: HashSet<acp::SessionModeId>,
    _settings_subscription: Subscription,
}

impl ModePickerDelegate {
    fn new(
        session_modes: Rc<dyn AgentSessionModes>,
        agent_server: Rc<dyn AgentServer>,
        fs: Arc<dyn Fs>,
        window: &mut Window,
        cx: &mut Context<ModeSelector>,
    ) -> Self {
        let agent_server_for_subscription = agent_server.clone();
        let settings_subscription =
            cx.observe_global_in::<SettingsStore>(window, move |picker, window, cx| {
                let new_favorites = agent_server_for_subscription.favorite_mode_ids(cx);
                if new_favorites != picker.delegate.favorites {
                    picker.delegate.favorites = new_favorites;
                    picker.refresh(window, cx);
                }
            });
        let favorites = agent_server.favorite_mode_ids(cx);

        Self {
            session_modes,
            agent_server,
            fs,
            filtered_entries: Vec::new(),
            selected_index: 0,
            selected_description: None,
            favorites,
            _settings_subscription: settings_subscription,
        }
    }

    pub fn cycle_favorite_modes(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if self.favorites.is_empty() {
            return;
        }

        let all_modes = self.session_modes.all_modes();
        let favorite_modes: Vec<_> = all_modes
            .iter()
            .filter(|mode| self.favorites.contains(&mode.id))
            .collect();

        if favorite_modes.is_empty() {
            return;
        }

        let current_id = self.session_modes.current_mode();
        let current_index_in_favorites = favorite_modes
            .iter()
            .position(|m| m.id == current_id)
            .unwrap_or(usize::MAX);

        let next_index = if current_index_in_favorites == usize::MAX {
            0
        } else {
            (current_index_in_favorites + 1) % favorite_modes.len()
        };

        let next_mode = favorite_modes[next_index].clone();
        self.session_modes
            .set_mode(next_mode.id.clone(), cx)
            .detach_and_log_err(cx);

        if let Some(new_index) = self.filtered_entries.iter().position(
            |entry| matches!(entry, ModePickerEntry::Mode(mode, _) if mode.id == next_mode.id),
        ) {
            self.set_selected_index(new_index, window, cx);
        } else {
            cx.notify();
        }
    }
}

fn build_filtered_entries(
    modes: &[acp::SessionMode],
    favorites: &HashSet<acp::SessionModeId>,
) -> Vec<ModePickerEntry> {
    let mut entries = Vec::new();

    let favorite_modes: Vec<_> = modes.iter().filter(|m| favorites.contains(&m.id)).collect();

    let has_favorites = !favorite_modes.is_empty();
    if has_favorites {
        entries.push(ModePickerEntry::Separator("Favorite".into()));
        for mode in favorite_modes {
            entries.push(ModePickerEntry::Mode(mode.clone(), true));
        }
        entries.push(ModePickerEntry::Separator("All".into()));
    }

    for mode in modes {
        let is_favorite = favorites.contains(&mode.id);
        entries.push(ModePickerEntry::Mode(mode.clone(), is_favorite));
    }

    entries
}

impl PickerDelegate for ModePickerDelegate {
    type ListItem = AnyElement;

    fn match_count(&self) -> usize {
        self.filtered_entries.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.selected_index = ix.min(self.filtered_entries.len().saturating_sub(1));
        cx.notify();
    }

    fn can_select(&self, ix: usize, _window: &mut Window, _cx: &mut Context<Picker<Self>>) -> bool {
        match self.filtered_entries.get(ix) {
            Some(ModePickerEntry::Mode(_, _)) => true,
            Some(ModePickerEntry::Separator(_)) | None => false,
        }
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Select a mode…".into()
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let all_modes = self.session_modes.all_modes();
        let favorites = self.favorites.clone();

        cx.spawn_in(window, async move |this, cx| {
            let filtered_modes = if query.is_empty() {
                all_modes
            } else {
                let candidates: Vec<_> = all_modes
                    .iter()
                    .enumerate()
                    .map(|(ix, mode)| StringMatchCandidate::new(ix, mode.name.as_ref()))
                    .collect();

                let matches = match_strings(
                    &candidates,
                    &query,
                    false,
                    true,
                    100,
                    &Default::default(),
                    cx.background_executor().clone(),
                )
                .await;

                matches
                    .into_iter()
                    .map(|mat| all_modes[mat.candidate_id].clone())
                    .collect()
            };

            this.update_in(cx, |this, window, cx| {
                this.delegate.filtered_entries =
                    build_filtered_entries(&filtered_modes, &favorites);

                let current_mode = this.delegate.session_modes.current_mode();
                let new_index = this
                    .delegate
                    .filtered_entries
                    .iter()
                    .position(|entry| {
                        matches!(entry, ModePickerEntry::Mode(mode, _) if mode.id == current_mode)
                    })
                    .unwrap_or(0);
                this.set_selected_index(new_index, Some(picker::Direction::Down), true, window, cx);
                cx.notify();
            })
            .ok();
        })
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(ModePickerEntry::Mode(mode, _)) = self.filtered_entries.get(self.selected_index)
        {
            if window.modifiers().secondary() {
                let default_mode = self.agent_server.default_mode(cx);
                let is_default = default_mode.as_ref() == Some(&mode.id);

                self.agent_server.set_default_mode(
                    if is_default {
                        None
                    } else {
                        Some(mode.id.clone())
                    },
                    self.fs.clone(),
                    cx,
                );
            }

            self.session_modes
                .set_mode(mode.id.clone(), cx)
                .detach_and_log_err(cx);

            cx.emit(DismissEvent);
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        cx.defer_in(window, |picker, window, cx| {
            picker.set_query("", window, cx);
        });
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        match self.filtered_entries.get(ix)? {
            ModePickerEntry::Separator(title) => {
                Some(crate::ui::ModelSelectorHeader::new(title, ix > 1).into_any_element())
            }
            ModePickerEntry::Mode(mode, is_favorite) => {
                let current_mode = self.session_modes.current_mode();
                let is_selected = mode.id == current_mode;
                let default_mode = self.agent_server.default_mode(cx);
                let is_default = default_mode.as_ref() == Some(&mode.id);

                let is_favorite = *is_favorite;
                let handle_action_click = {
                    let mode_id = mode.id.clone();
                    let fs = self.fs.clone();
                    let agent_server = self.agent_server.clone();

                    cx.listener(move |_, _, _, cx| {
                        agent_server.toggle_favorite_mode(
                            mode_id.clone(),
                            !is_favorite,
                            fs.clone(),
                            cx,
                        );
                    })
                };

                Some(
                    div()
                        .id(("mode-picker-menu-child", ix))
                        .when_some(mode.description.clone(), |this, description| {
                            this.on_hover(cx.listener(move |menu, hovered, _, cx| {
                                if *hovered {
                                    menu.delegate.selected_description =
                                        Some((ix, description.clone().into(), is_default));
                                } else if matches!(menu.delegate.selected_description, Some((id, _, _)) if id == ix)
                                {
                                    menu.delegate.selected_description = None;
                                }
                                cx.notify();
                            }))
                        })
                        .child(
                            crate::ui::ModelSelectorListItem::new(ix, mode.name.clone())
                                .is_selected(is_selected)
                                .is_focused(selected)
                                .is_favorite(is_favorite)
                                .on_toggle_favorite(handle_action_click),
                        )
                        .into_any_element(),
                )
            }
        }
    }

    fn documentation_aside(
        &self,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<DocumentationAside> {
        self.selected_description
            .as_ref()
            .map(|(_, description, is_default)| {
                let description = description.clone();
                let is_default = *is_default;

                let settings = AgentSettings::get_global(cx);
                let side = match settings.dock {
                    settings::DockPosition::Left => DocumentationSide::Right,
                    settings::DockPosition::Bottom | settings::DockPosition::Right => {
                        DocumentationSide::Left
                    }
                };

                DocumentationAside::new(
                    side,
                    Rc::new(move |_| {
                        v_flex()
                            .gap_1()
                            .child(Label::new(description.clone()))
                            .child(HoldForDefault::new(is_default))
                            .into_any_element()
                    }),
                )
            })
    }

    fn documentation_aside_index(&self) -> Option<usize> {
        self.selected_description.as_ref().map(|(ix, _, _)| *ix)
    }

    fn render_footer(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        None
    }
}

#[derive(IntoElement)]
pub struct ModeSelectorTooltip {
    show_cycle_row: bool,
}

impl ModeSelectorTooltip {
    pub fn new() -> Self {
        Self {
            show_cycle_row: true,
        }
    }

    pub fn show_cycle_row(mut self, show: bool) -> Self {
        self.show_cycle_row = show;
        self
    }
}

impl RenderOnce for ModeSelectorTooltip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .child(Label::new("Change Mode"))
                    .child(ui::KeyBinding::for_action(&ToggleProfileSelector, cx)),
            )
            .when(self.show_cycle_row, |this| {
                this.child(
                    h_flex()
                        .pt_1()
                        .gap_2()
                        .border_t_1()
                        .border_color(cx.theme().colors().border_variant)
                        .justify_between()
                        .child(Label::new("Cycle Favorited Modes"))
                        .child(ui::KeyBinding::for_action(&CycleFavoriteModes, cx)),
                )
            })
    }
}
